use crate::types::{BuildMeta, CliResponse};
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
            json!({
                "id": a.get("id").and_then(|v| v.as_str()).unwrap_or("?"),
                "name": a.get("name").and_then(|v| v.as_str()).unwrap_or("(unnamed)"),
                "status": a.get("status").and_then(|v| v.as_str()).unwrap_or("unknown"),
                "execution_mode": a.get("execution_mode").and_then(|v| v.as_str()).unwrap_or("focused"),
                "schedule_type": a.get("schedule").and_then(|s| s.get("schedule_type")).and_then(|v| v.as_str()).unwrap_or("event"),
                "execution_count": a.get("execution_count").and_then(|v| v.as_u64()).unwrap_or(0),
                "success_count": a.get("success_count").and_then(|v| v.as_u64()).unwrap_or(0),
                "error_count": a.get("error_count").and_then(|v| v.as_u64()).unwrap_or(0),
                "avg_duration_ms": a.get("avg_duration_ms").and_then(|v| v.as_u64()).unwrap_or(0),
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

/// Create a new agent
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
    device_ids: Option<&str>,
    resources: Option<&str>,
    metrics: Option<&str>,
    commands: Option<&str>,
    enable_tool_chaining: Option<bool>,
    max_chain_depth: Option<usize>,
    priority: Option<u8>,
    context_window_size: Option<usize>,
) -> Result<CliResponse> {
    let schedule_type_val = schedule_type.unwrap_or("event");
    let mut schedule = json!({
        "schedule_type": schedule_type_val,
    });
    match schedule_type_val {
        "interval" => {
            if let Some(config) = schedule_config {
                if let Ok(secs) = config.parse::<u64>() {
                    schedule["interval_seconds"] = json!(secs);
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

    let exec_mode = execution_mode.unwrap_or("free");
    let has_resources = resources.is_some() || device_ids.map(|d| !d.is_empty()).unwrap_or(false);

    let mut body = json!({
        "name": name,
        "user_prompt": user_prompt,
        "schedule": schedule,
        "execution_mode": exec_mode,
    });
    if let Some(desc) = description {
        body["description"] = json!(desc);
    }
    if let Some(backend) = llm_backend {
        body["llm_backend_id"] = json!(backend);
    }
    if let Some(prompt) = system_prompt {
        body["system_prompt"] = json!(prompt);
    }
    if let Some(ids) = device_ids {
        let id_list: Vec<&str> = ids
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if !id_list.is_empty() {
            body["device_ids"] = json!(id_list);
        }
    }
    if let Some(res_str) = resources {
        if let Ok(res_val) = serde_json::from_str::<serde_json::Value>(res_str) {
            body["resources"] = res_val;
        }
    }
    if let Some(etc) = enable_tool_chaining {
        body["enable_tool_chaining"] = json!(etc);
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
        if let Ok(metrics_val) = serde_json::from_str::<serde_json::Value>(metrics_str) {
            body["metrics"] = metrics_val;
        }
    }
    if let Some(commands_str) = commands {
        if let Ok(commands_val) = serde_json::from_str::<serde_json::Value>(commands_str) {
            body["commands"] = commands_val;
        }
    }

    // focused mode requires resources
    if exec_mode == "focused" && !has_resources {
        // Surface a concrete invocation so the caller can self-correct instead
        // of retrying blind. Matches the data_push error_with_suggestion gold
        // standard.
        return Ok(CliResponse::error_with_suggestion(
            "Focused mode requires --resources or --device-ids to bind at least one resource.",
            "FOCUSED_REQUIRES_RESOURCES",
            "Example: --device-ids device-001,device-002  (binds the focused agent to those devices; or pass --resources with a JSON array)",
        ));
    }

    let data = client.post("/agents", &body).await?;
    let agent_id = data
        .get("data")
        .and_then(|d| d.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let meta = BuildMeta {
        r#type: "agent".to_string(),
        action: "create".to_string(),
        entity_id: agent_id.clone(),
        entity_name: Some(name.to_string()),
        undo_command: format!("neomind agent delete {}", agent_id),
    };

    Ok(CliResponse::success_with_meta(
        data,
        // Agents are created Paused — they never run until activated. The
        // follow-up command is the difference between a finished workflow
        // and a "created it but nothing happened" failure mode (a top eval
        // failure class), so the receipt teaches it explicitly.
        &format!(
            "Agent created (status: paused — it will NOT run until activated). \
             Next: neomind agent control {} active",
            agent_id
        ),
        meta,
    ))
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
    execution_mode: Option<&str>,
    device_ids: Option<&str>,
    resources: Option<&str>,
    metrics: Option<&str>,
    commands: Option<&str>,
    enable_tool_chaining: Option<bool>,
    max_chain_depth: Option<usize>,
    priority: Option<u8>,
    context_window_size: Option<usize>,
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
    if let Some(st) = schedule_type {
        let mut schedule = json!({"schedule_type": st});
        match st {
            "interval" => {
                if let Some(config) = schedule_config {
                    if let Ok(secs) = config.parse::<u64>() {
                        schedule["interval_seconds"] = json!(secs);
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
        body["schedule"] = schedule;
    }
    if let Some(em) = execution_mode {
        body["execution_mode"] = json!(em);
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
        if let Ok(res_val) = serde_json::from_str::<serde_json::Value>(res_str) {
            body["resources"] = res_val;
        }
    }
    if let Some(etc) = enable_tool_chaining {
        body["enable_tool_chaining"] = json!(etc);
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
        if let Ok(metrics_val) = serde_json::from_str::<serde_json::Value>(metrics_str) {
            body["metrics"] = metrics_val;
        }
    }
    if let Some(commands_str) = commands {
        if let Ok(commands_val) = serde_json::from_str::<serde_json::Value>(commands_str) {
            body["commands"] = commands_val;
        }
    }

    let data = client.put(&format!("/agents/{}", id), &body).await?;
    Ok(CliResponse::success(data, "Agent updated"))
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

/// Invoke agent with input
pub async fn invoke_agent(client: &ApiClient, id: &str, input: &str) -> Result<CliResponse> {
    let id = resolve_agent_id(client, id).await?;
    let body = json!({ "input": input });
    let data = client
        .post(&format!("/agents/{}/invoke", id), &body)
        .await?;
    Ok(CliResponse::success(data, "Agent invoked"))
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

/// Send message to agent
pub async fn send_message(
    client: &ApiClient,
    id: &str,
    message: &str,
    message_type: Option<&str>,
) -> Result<CliResponse> {
    let id = resolve_agent_id(client, id).await?;
    let mut body = json!({
        "content": message,
    });
    if let Some(mt) = message_type {
        body["type"] = json!(mt);
    }
    let data = client
        .post(&format!("/agents/{}/messages", id), &body)
        .await?;
    Ok(CliResponse::success(data, "Message sent"))
}
