/// Helper function to extract an array from a JSON value, handling both direct arrays
/// and truncated nested structures ({"items": [...], "_total_count": N, ...})
pub(crate) fn extract_array(
    json_value: &serde_json::Value,
    key: &str,
) -> Option<Vec<serde_json::Value>> {
    // First try to get the key directly as an array
    if let Some(arr) = json_value.get(key).and_then(|v| v.as_array()) {
        return Some(arr.clone());
    }

    // Then try to get it from a truncated structure
    if let Some(obj) = json_value.get(key).and_then(|v| v.as_object()) {
        if let Some(items) = obj.get("items").and_then(|i| i.as_array()) {
            return Some(items.clone());
        }
    }

    None
}

/// Format results from CLI domain tools (device, agent, rule, message, extension)
/// by detecting the JSON structure. Handles structured JSON results from CLI commands.
pub(crate) fn format_cli_tool_result(
    tool_name: &str,
    json: &serde_json::Value,
    response: &mut String,
) {
    // Detect what kind of result this is based on JSON structure

    // Agent list: has "agents" key with array or nested object
    if json.get("agents").is_some() || json.get("count").is_some() && tool_name == "agent" {
        format_agent_list(json, response);
        return;
    }

    // Device list: has "devices" array
    if let Some(devices) = extract_array(json, "devices") {
        response.push_str(&format!("## Device List ({} total)\n\n", devices.len()));
        for device in devices {
            let name = device
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown");
            let id = device.get("id").and_then(|i| i.as_str()).unwrap_or("");
            let device_type = device
                .get("type")
                .or_else(|| device.get("device_type"))
                .and_then(|t| t.as_str())
                .unwrap_or("unknown");
            let status = device.get("status").and_then(|s| s.as_str()).unwrap_or("");

            if status.is_empty() {
                response.push_str(&format!("- **{}** ({}) - {}\n", name, id, device_type));
            } else {
                response.push_str(&format!(
                    "- **{}** ({}) - {} - {}\n",
                    name, id, device_type, status
                ));
            }
        }
        return;
    }

    // Device query result: has "device_id" and "points"
    if json.get("device_id").is_some() && json.get("points").is_some() {
        let device_id = json
            .get("device_id")
            .and_then(|d| d.as_str())
            .unwrap_or("unknown");
        let metric = json
            .get("metric")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown");
        let points = json.get("points").and_then(|p| p.as_array());

        response.push_str(&format!("## {} - {}\n\n", device_id, metric));

        if let Some(pts) = points {
            if pts.is_empty() {
                response.push_str("No data available.\n");
            } else {
                for point in pts.iter().take(10) {
                    let ts = point.get("timestamp").and_then(|t| t.as_i64()).unwrap_or(0);
                    let is_image = point.get("base64_data").is_some();
                    let value = if is_image {
                        "[image data]".to_string()
                    } else {
                        point
                            .get("value")
                            .map(|v| v.to_string().trim_matches('"').to_string())
                            .unwrap_or_else(|| "N/A".to_string())
                    };

                    if ts > 0 {
                        let time_str = chrono::DateTime::from_timestamp(ts, 0)
                            .map(|dt| dt.format("%H:%M:%S").to_string())
                            .unwrap_or_else(|| ts.to_string());
                        response.push_str(&format!("- {}: {}\n", time_str, value));
                    } else {
                        response.push_str(&format!("- {}\n", value));
                    }
                }
                if pts.len() > 10 {
                    response.push_str(&format!("\n... ({} more data points)\n", pts.len() - 10));
                }
            }
        }
        return;
    }

    // Device get with metrics: has "id"/"name" + "type" + "metrics" array with values
    if json.get("name").is_some() && json.get("type").is_some() {
        if let Some(metrics) = json.get("metrics").and_then(|m| m.as_array()) {
            let name = json
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown");
            let device_type = json
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("unknown");
            response.push_str(&format!("## {} ({})\n\n", name, device_type));

            for metric in metrics {
                let display_name = metric
                    .get("display_name")
                    .and_then(|d| d.as_str())
                    .or_else(|| metric.get("name").and_then(|n| n.as_str()))
                    .unwrap_or("unknown");
                let unit = metric.get("unit").and_then(|u| u.as_str()).unwrap_or("");

                if let Some(value) = metric.get("value") {
                    let value_str = value.to_string().trim_matches('"').to_string();
                    if unit.is_empty() {
                        response.push_str(&format!("- **{}**: {}\n", display_name, value_str));
                    } else {
                        response
                            .push_str(&format!("- **{}**: {} {}\n", display_name, value_str, unit));
                    }
                } else {
                    response.push_str(&format!("- **{}**: 无数据\n", display_name));
                }
            }
            return;
        }
    }

    // Metric not found with suggestions: has "error" + "available_metrics"
    if json.get("error").is_some() && json.get("available_metrics").is_some() {
        let error = json
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("Unknown error");
        response.push_str(&format!("**Error**: {}\n\n", error));

        if let Some(available) = json.get("available_metrics").and_then(|a| a.as_array()) {
            response.push_str("**Available metrics:**\n");
            for metric in available {
                let name = metric.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let display_name = metric
                    .get("display_name")
                    .and_then(|d| d.as_str())
                    .unwrap_or("");
                let unit = metric.get("unit").and_then(|u| u.as_str()).unwrap_or("");
                if display_name.is_empty() {
                    response.push_str(&format!("- `{}`\n", name));
                } else if unit.is_empty() {
                    response.push_str(&format!("- `{}` ({})\n", name, display_name));
                } else {
                    response.push_str(&format!("- `{}` ({}) - {}\n", name, display_name, unit));
                }
            }
        }
        return;
    }

    // Rule list: has "rules" array
    if let Some(rules) = extract_array(json, "rules") {
        response.push_str(&format!("## Automation Rules ({} total)\n\n", rules.len()));
        for rule in rules {
            let name = rule
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown");
            // Support both new "status" string field and legacy "enabled" boolean
            let status_display = if let Some(status) = rule.get("status").and_then(|s| s.as_str()) {
                match status {
                    "active" => "[Active]",
                    "paused" => "[Paused]",
                    "triggered" => "[Triggered]",
                    "disabled" => "[Disabled]",
                    _ => status,
                }
            } else if rule
                .get("enabled")
                .and_then(|e| e.as_bool())
                .unwrap_or(false)
            {
                "[Active]"
            } else {
                "[Disabled]"
            };
            let desc = rule
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("");
            if desc.is_empty() {
                response.push_str(&format!("- **{}** {}\n", name, status_display));
            } else {
                response.push_str(&format!("- **{}** {} -- {}\n", name, status_display, desc));
            }
        }
        return;
    }

    // Alert list: has "alerts" array
    if let Some(alerts) = extract_array(json, "alerts") {
        response.push_str(&format!("## Alerts ({} total)\n\n", alerts.len()));
        for alert in alerts.iter().take(10) {
            let title = alert
                .get("title")
                .or_else(|| alert.get("message"))
                .and_then(|t| t.as_str())
                .unwrap_or("unknown");
            let severity = alert
                .get("severity")
                .and_then(|s| s.as_str())
                .unwrap_or("info");
            let tag = match severity {
                "critical" => "[CRITICAL]",
                "warning" => "[WARN]",
                _ => "[INFO]",
            };
            response.push_str(&format!("- {} **{}** ({})\n", tag, title, severity));
        }
        if alerts.len() > 10 {
            response.push_str(&format!("\n... ({} more alerts)\n", alerts.len() - 10));
        }
        return;
    }

    // Extension list: has "extensions" array
    if let Some(extensions) = extract_array(json, "extensions") {
        response.push_str(&format!("## Extensions ({} total)\n\n", extensions.len()));
        for ext in extensions {
            let name = ext
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown");
            let status = ext
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown");
            let tag = if status == "running" {
                "[running]"
            } else {
                "[stopped]"
            };
            response.push_str(&format!("- {} **{}** ({})\n", tag, name, status));
        }
        return;
    }

    // Agent details: has "name" and "type" at top level (single agent)
    if json.get("name").is_some() && json.get("type").is_some() && tool_name == "agent" {
        let name = json
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("unknown");
        let status = json
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("unknown");
        response.push_str(&format!("## Agent: {}\n\n", name));
        response.push_str(&format!("**Status**: {}\n", status));
        if let Some(stats) = json.get("stats") {
            if let Some(total) = stats.get("total_executions").and_then(|t| t.as_u64()) {
                response.push_str(&format!("**Total Executions**: {}\n", total));
            }
        }
        return;
    }

    // Agent execution history: has "agent_id" + "stats"
    if json.get("agent_id").is_some() && json.get("stats").is_some() {
        if let Some(stats) = json.get("stats") {
            let total = stats
                .get("total_executions")
                .and_then(|t| t.as_u64())
                .unwrap_or(0);
            let success = stats
                .get("successful_executions")
                .and_then(|s| s.as_u64())
                .unwrap_or(0);
            let failed = stats
                .get("failed_executions")
                .and_then(|f| f.as_u64())
                .unwrap_or(0);
            let avg_ms = stats
                .get("avg_duration_ms")
                .and_then(|d| d.as_u64())
                .unwrap_or(0);
            response.push_str("## Execution Stats\n\n");
            response.push_str(&format!("- **Total**: {} times\n", total));
            response.push_str(&format!(
                "- **Success**: {} | **Failed**: {}\n",
                success, failed
            ));
            if avg_ms > 0 {
                let avg_sec = avg_ms as f64 / 1000.0;
                response.push_str(&format!("- **Avg Duration**: {:.1}s\n", avg_sec));
            }
            if let Some(last_ms) = stats.get("last_duration_ms").and_then(|d| d.as_u64()) {
                if last_ms > 0 {
                    response.push_str(&format!(
                        "- **Last Duration**: {:.1}s\n",
                        last_ms as f64 / 1000.0
                    ));
                }
            }
        }
        return;
    }

    // Message/alert list: has "count" and "messages" array with message objects (id, title, level)
    if let Some(messages) = extract_array(json, "messages") {
        // Distinguish from agent conversation history:
        // message tool returns objects with "title", "level", "read" fields
        // agent conversation returns objects with "role", "content" fields
        let is_message_list = messages.first().is_some_and(|m| {
            m.get("title").is_some() || m.get("level").is_some() || m.get("read").is_some()
        });

        if is_message_list {
            let count = json
                .get("count")
                .and_then(|c| c.as_u64())
                .unwrap_or(messages.len() as u64);
            response.push_str(&format!("## Messages & Alerts ({} total)\n\n", count));
            for msg in messages.iter().take(15) {
                let title = msg
                    .get("title")
                    .and_then(|t| t.as_str())
                    .unwrap_or("unknown");
                let level = msg.get("level").and_then(|l| l.as_str()).unwrap_or("info");
                let read = msg.get("read").and_then(|r| r.as_bool()).unwrap_or(false);
                let id = msg.get("id").and_then(|i| i.as_str()).unwrap_or("");

                let icon = match level {
                    "urgent" | "critical" => "[CRITICAL]",
                    "important" => "[IMPORTANT]",
                    "notice" | "warning" => "[WARN]",
                    _ => "[INFO]",
                };
                let read_icon = if read { "[read]" } else { "[unread]" };
                response.push_str(&format!(
                    "{} {} [{}] {} (`{}`)\n",
                    icon,
                    read_icon,
                    level,
                    title,
                    &id[..8.min(id.len())]
                ));
            }
            if messages.len() > 15 {
                response.push_str(&format!("\n... ({} more)\n", messages.len() - 15));
            }
            return;
        }
    }

    // Agent conversation history: has "messages" array with role/content
    if let Some(messages) = extract_array(json, "messages") {
        response.push_str(&format!(
            "## Conversation Log ({} messages)\n\n",
            messages.len()
        ));
        for msg in messages.iter().take(10) {
            let role = msg
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or("unknown");
            let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
            let preview: String = content.chars().take(100).collect();
            response.push_str(&format!("- **{}**: {}\n", role, preview));
        }
        if messages.len() > 10 {
            response.push_str(&format!("\n... ({} more messages)\n", messages.len() - 10));
        }
        return;
    }

    // Control/execution success — but check if there's meaningful data first
    if json.get("success").is_some()
        || json.get("execution_id").is_some()
        || json.get("rule_id").is_some()
    {
        // If there's a "data" object with useful fields, format those instead of generic message
        if let Some(data) = json.get("data") {
            format_json_data(data, response);
            return;
        }
        if let Some(exec_id) = json.get("execution_id").and_then(|e| e.as_str()) {
            response.push_str(&format!("[OK] Executed successfully (ID: {})\n", exec_id));
        } else if let Some(rule_id) = json.get("rule_id").and_then(|r| r.as_str()) {
            response.push_str(&format!("[OK] Created successfully (ID: {})\n", rule_id));
        } else if let Some(agent_id) = json
            .get("agent_id")
            .or_else(|| json.get("id"))
            .and_then(|a| a.as_str())
        {
            response.push_str(&format!("[OK] Created successfully (ID: {})\n", agent_id));
        } else if json
            .get("success")
            .and_then(|s| s.as_bool())
            .unwrap_or(false)
        {
            response.push_str(&format!("**[OK]** {} operation succeeded\n", tool_name));
        } else {
            // Has error
            let error = json
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("Unknown error");
            response.push_str(&format!("!! {} failed: {}\n", tool_name, error));
        }
        return;
    }

    // Fallback: format the JSON object with key-value pairs (handles extension tools, etc.)
    if json.is_object() || json.is_array() {
        format_json_data(json, response);
    } else {
        response.push_str(&format!("**[OK]** {} completed.\n", tool_name));
    }
}

/// Format agent list from JSON result.
pub(crate) fn format_agent_list(json: &serde_json::Value, response: &mut String) {
    let agents_array = if let Some(agents_obj) = json.get("agents").and_then(|a| a.as_object()) {
        agents_obj.get("items").and_then(|i| i.as_array())
    } else {
        json.get("agents").and_then(|a| a.as_array())
    };

    if let Some(agents) = agents_array {
        if agents.is_empty() {
            response.push_str("**AI Agent List**\n\nNo AI Agents configured.");
        } else {
            response.push_str(&format!("**AI Agent List** ({} total)\n\n", agents.len()));
            for agent in agents {
                let name = agent
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("unknown");
                let status = agent
                    .get("status")
                    .and_then(|s| s.as_str())
                    .unwrap_or("unknown");
                let icon = match status {
                    "active" | "Active" => "[on]",
                    _ => "[off]",
                };
                response.push_str(&format!("- {} **{}** ({})\n", icon, name, status));

                if let Some(desc) = agent.get("description").and_then(|d| d.as_str()) {
                    if !desc.is_empty() && desc != "null" {
                        response.push_str(&format!("  {}\n", desc));
                    }
                }
            }
        }
    } else if let Some(count) = json.get("count").and_then(|c| c.as_u64()) {
        response.push_str(&format!("**AI Agent List** ({} total)\n", count));
    } else {
        response.push_str("**AI Agent List**\n\nNo AI Agents found.");
    }
}

/// Format a generic JSON data object into readable key-value pairs.
/// Used for extension tool results (weather, image analysis, etc.)
pub(crate) fn format_json_data(data: &serde_json::Value, response: &mut String) {
    if let Some(obj) = data.as_object() {
        for (key, value) in obj {
            // Skip nested objects and arrays in simple view
            if value.is_object() || value.is_array() {
                continue;
            }

            // Convert snake_case to Title Case
            let display_name: String = key
                .chars()
                .enumerate()
                .flat_map(|(i, c)| {
                    if i == 0 {
                        c.to_uppercase().collect::<Vec<char>>()
                    } else if c == '_' {
                        vec![' ']
                    } else {
                        vec![c]
                    }
                })
                .collect();

            let value_str = match value {
                serde_json::Value::Bool(b) => {
                    if *b {
                        "Yes".to_string()
                    } else {
                        "No".to_string()
                    }
                }
                serde_json::Value::Number(n) => {
                    if key.ends_with("_c") {
                        format!("{}°C", n)
                    } else if key.ends_with("_percent") {
                        format!("{}%", n)
                    } else if key.ends_with("_kmph") {
                        format!("{} km/h", n)
                    } else if key.ends_with("_hpa") {
                        format!("{} hPa", n)
                    } else if key.ends_with("_ms") || key.ends_with("_duration_ms") {
                        format!("{:.1}s", n.as_f64().unwrap_or(0.0) / 1000.0)
                    } else {
                        n.to_string()
                    }
                }
                serde_json::Value::String(s) => s.clone(),
                _ => value.to_string(),
            };

            response.push_str(&format!("- **{}**: {}\n", display_name, value_str));
        }
    } else if let Some(arr) = data.as_array() {
        for (i, item) in arr.iter().enumerate().take(10) {
            response.push_str(&format!("{}. {}\n", i + 1, item));
        }
        if arr.len() > 10 {
            response.push_str(&format!("\n... ({} more)\n", arr.len() - 10));
        }
    } else {
        response.push_str(&format!("{}\n", data));
    }
}

/// Format tool results into a user-friendly response
/// This avoids calling the LLM again after tool execution, preventing excessive thinking
pub fn format_tool_results(tool_results: &[(String, String)]) -> String {
    if tool_results.is_empty() {
        return "操作已完成。".to_string();
    }

    let mut response = String::new();

    for (tool_name, result) in tool_results {
        // Try to parse the result as JSON for better formatting
        if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(result) {
            // All tool results share the same JSON structure detection.
            // Shell tool (and CLI domains routed to shell) return CliResponse JSON
            // with "command"/"stdout"/"exit_code" keys. Other tools return
            // domain-specific JSON (devices, rules, agents, etc.).
            if json_value.get("command").is_some()
                && (json_value.get("stdout").is_some() || json_value.get("exit_code").is_some())
            {
                // Shell/CliResponse format
                let cmd = json_value
                    .get("command")
                    .and_then(|c| c.as_str())
                    .unwrap_or("?");
                let desc = json_value.get("description").and_then(|d| d.as_str());
                if let Some(desc) = desc {
                    response.push_str(&format!(
                        "## {}: {}\n**Command**: `{}`\n",
                        tool_name, desc, cmd
                    ));
                } else {
                    response.push_str(&format!("## `{}` ({})\n", tool_name, cmd));
                }
                if json_value
                    .get("timed_out")
                    .and_then(|t| t.as_bool())
                    .unwrap_or(false)
                {
                    response.push_str("**Timed out**\n");
                }
                if let Some(exit_code) = json_value.get("exit_code") {
                    response.push_str(&format!("**Exit code**: {}\n", exit_code));
                }
                if let Some(stdout) = json_value.get("stdout").and_then(|s| s.as_str()) {
                    if !stdout.is_empty() {
                        response.push_str(&format!("```\n{}\n```\n", stdout));
                    }
                }
                if let Some(stderr) = json_value.get("stderr").and_then(|s| s.as_str()) {
                    if !stderr.is_empty() {
                        response.push_str(&format!("**stderr:**\n```\n{}\n```\n", stderr));
                    }
                }
            } else {
                // Non-shell JSON — detect structure for formatting
                format_cli_tool_result(tool_name, &json_value, &mut response);
            }
        } else {
            // Result is not valid JSON, use as-is
            // Use a structured format with result prefix to prevent LLM hallucination
            // of tool results (model can learn the simple "tool executed" pattern)
            // Show more for error messages to preserve diagnostic info
            let is_error = result.starts_with("Error:");
            let max_chars = if is_error { 500 } else { 80 };
            let preview: String = result.chars().take(max_chars).collect();
            response.push_str(&format!("**[ToolResult:{}]** {}\n", tool_name, preview));
        }
    }

    if response.ends_with('\n') {
        response.pop();
    }

    // Safe character-based slicing for logging
    let preview: String = response.chars().take(200).collect();
    tracing::info!(
        "format_tool_results: Final output length: {} chars, preview: {}",
        response.len(),
        preview
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Empty result set gets a neutral completion line, never an empty bubble.
    #[test]
    fn empty_results_get_completion_line() {
        assert_eq!(format_tool_results(&[]), "操作已完成。");
    }

    /// Shell/CliResponse JSON (command + stdout/exit_code) renders as a
    /// command block with exit code and output — the main path for the
    /// chat agent's `shell` tool.
    #[test]
    fn shell_response_renders_command_block() {
        let result = serde_json::json!({
            "success": true,
            "command": "neomind device list",
            "exit_code": 0,
            "stdout": "total: 2\nonline: 1"
        })
        .to_string();
        let out = format_tool_results(&[("shell".to_string(), result)]);
        assert!(out.contains("neomind device list"), "cmd missing: {out}");
        assert!(out.contains("Exit code**: 0"), "exit code missing: {out}");
        assert!(out.contains("total: 2"), "stdout missing: {out}");
    }

    /// A failed shell call must surface stderr, not look like success.
    #[test]
    fn shell_failure_surfaces_stderr() {
        let result = serde_json::json!({
            "success": false,
            "command": "neomind device get x",
            "exit_code": 1,
            "stdout": "",
            "stderr": "API error (401): Authentication required"
        })
        .to_string();
        let out = format_tool_results(&[("shell".to_string(), result)]);
        assert!(out.contains("stderr"), "stderr marker missing: {out}");
        assert!(out.contains("401"), "stderr content missing: {out}");
    }

    /// timed_out flag is reported — a hang must never read as a clean run.
    #[test]
    fn shell_timeout_is_flagged() {
        let result = serde_json::json!({
            "command": "long-task",
            "exit_code": 124,
            "stdout": "",
            "timed_out": true
        })
        .to_string();
        let out = format_tool_results(&[("shell".to_string(), result)]);
        assert!(out.contains("Timed out"), "timeout flag missing: {out}");
    }

    /// Non-JSON text renders as a prefixed preview. Errors keep a LONG
    /// preview (diagnostics survive); success output is truncated hard
    /// (80 chars) to keep the bubble small.
    #[test]
    fn non_json_error_gets_long_preview_success_short() {
        let err = format!("Error: {}", "x".repeat(600));
        let out = format_tool_results(&[("t".to_string(), err.clone())]);
        assert!(out.contains("[ToolResult:t]"), "prefix missing: {out}");
        let kept_x = out.matches('x').count();
        assert!(
            (400..=500).contains(&kept_x),
            "error preview should keep up to 500 chars, kept {kept_x}"
        );

        let ok = "y".repeat(600);
        let out = format_tool_results(&[("t".to_string(), ok)]);
        let kept_y = out.matches('y').count();
        assert_eq!(kept_y, 80, "success preview must truncate at 80 chars");
    }

    /// Multi-byte safety: truncation is char-based, never panics or splits
    /// a UTF-8 scalar mid-byte (CJK output is the norm, not the exception).
    #[test]
    fn multibyte_output_truncates_on_char_boundaries() {
        let ok = "设".repeat(200);
        let out = format_tool_results(&[("t".to_string(), ok)]);
        assert!(out.contains('设'));
        assert_eq!(out.matches('设').count(), 80);
    }

    /// Multiple tool calls render in order, all present.
    #[test]
    fn multiple_results_render_in_order() {
        let a = serde_json::json!({"command": "cmd-a", "exit_code": 0}).to_string();
        let b = serde_json::json!({"command": "cmd-b", "exit_code": 0}).to_string();
        let out = format_tool_results(&[("shell".to_string(), a), ("shell".to_string(), b)]);
        let (pos_a, pos_b) = (out.find("cmd-a").unwrap(), out.find("cmd-b").unwrap());
        assert!(pos_a < pos_b, "results must preserve call order");
    }

    /// Domain JSON (non-shell) routes to the structured formatter: a real
    /// `neomind device list` payload renders as a human-readable list.
    #[test]
    fn device_list_json_renders_device_list() {
        let result = serde_json::json!({
            "devices": [
                {"name": "NE101-camera", "id": "dev-1", "device_type": "ne101", "status": "online"},
                {"name": "AM102", "id": "dev-2", "device_type": "milesight_am102", "status": "offline"}
            ]
        })
        .to_string();
        let out = format_tool_results(&[("device".to_string(), result)]);
        assert!(out.contains("Device List"), "header missing: {out}");
        assert!(out.contains("NE101-camera"), "device missing: {out}");
        assert!(out.contains("online"), "status missing: {out}");
    }
}
