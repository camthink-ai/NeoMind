use crate::types::CliResponse;
use crate::ApiClient;
use anyhow::Result;
use serde_json::{json, Value};

/// List all extensions with compact summary.
///
/// Returns id, name, version, runtime status, run `enabled`, AI
/// `ai_enabled`, and the next discovery step (`extension commands --search`
/// or `extension commands <id>`).
/// `ai_enabled` is the Chat/Agent master switch; `enabled` is run/load.
pub async fn list_extensions(client: &ApiClient) -> Result<CliResponse> {
    let data = client.get("/extensions").await?;

    let extensions = data
        .as_array()
        .or_else(|| data.get("extensions").and_then(|v| v.as_array()))
        .or_else(|| {
            data.get("data").and_then(|d| d.as_array()).or_else(|| {
                data.get("data")
                    .and_then(|d| d.get("extensions"))
                    .and_then(|v| v.as_array())
            })
        });

    let Some(extensions) = extensions else {
        return Ok(CliResponse::success(data, "Extensions listed"));
    };

    // Bound large catalogs so Chat context stays usable; full dump via get.
    const LIST_CAP: usize = 50;
    let total = extensions.len();
    let truncated = total > LIST_CAP;
    let summary: Vec<serde_json::Value> = extensions
        .iter()
        .take(LIST_CAP)
        .map(|e| {
            let id = e.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            let status = e
                .get("status")
                .and_then(|v| v.as_str())
                .or_else(|| e.get("state").and_then(|v| v.as_str()))
                .unwrap_or("unknown");
            let enabled = e.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
            let ai_enabled = e
                .get("ai_enabled")
                .and_then(|v| v.as_bool())
                // Pre-split APIs only exposed `enabled` as the AI switch.
                .unwrap_or(enabled);
            let status_l = status.to_ascii_lowercase();
            let running = e
                .get("running")
                .and_then(|v| v.as_bool())
                .or_else(|| e.get("is_running").and_then(|v| v.as_bool()))
                .unwrap_or_else(|| {
                    // API may return "Running (Isolated)" / "running" / health ok.
                    status_l.contains("running")
                        || status_l == "active"
                        || status_l == "ok"
                        || status_l == "healthy"
                });
            json!({
                "id": id,
                "name": e.get("name").and_then(|v| v.as_str()).unwrap_or("(unnamed)"),
                "version": e.get("version").and_then(|v| v.as_str()).unwrap_or("?"),
                "status": status,
                "running": running,
                "enabled": enabled,
                "ai_enabled": ai_enabled,
                "description": e.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                "next": format!("concrete task → neomind extension commands --search \"…\""),
            })
        })
        .collect();

    let mut resp = CliResponse::success(
        json!({
            "total": total,
            "returned": summary.len(),
            "truncated": truncated,
            "extensions": summary,
        }),
        format!("{} extension(s) listed", total),
    );
    if total == 0 {
        resp = resp.with_suggestion(
            "No extensions installed. Install from the marketplace or a .nep file, then re-list.",
        );
    } else if truncated {
        resp = resp.with_suggestion(format!(
            "Showing first {LIST_CAP} of {total} (AI-visible overview). Concrete task → \
neomind extension commands --search \"<task>\". Do not search for \"extension capabilities\"."
        ));
    } else {
        resp = resp.with_suggestion(
            "Compact AI-visible overview. Concrete task → neomind extension commands --search \"<task>\" \
(use hit shell_example). Do not search for \"extension capabilities\".",
        );
    }
    Ok(resp)
}

/// Percent-encode a URL path segment. A raw `/` inside an extension ID
/// splits the request path into extra segments, which escapes the
/// `/extensions/{id}` route entirely — the web SPA fallback then answers
/// 200 with HTML, and the API client reads that as success.
fn encode_segment(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    for b in id.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Get extension by ID
pub async fn get_extension(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client
        .get(&format!("/extensions/{}", encode_segment(id)))
        .await?;
    Ok(CliResponse::success(data, "Extension retrieved"))
}

/// Get extension health status
pub async fn get_extension_status(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let resp = client
        .get(&format!("/extensions/{}/health", encode_segment(id)))
        .await?;
    // Extract inner data to avoid double-wrapping (API already has success/data structure)
    let data = resp.get("data").cloned().unwrap_or(resp);
    Ok(CliResponse::success(data, "Extension status retrieved"))
}

/// Get extension logs
pub async fn get_extension_logs(
    client: &ApiClient,
    id: &str,
    lines: Option<usize>,
) -> Result<CliResponse> {
    let path = if let Some(n) = lines {
        format!("/extensions/{}/logs?lines={}", encode_segment(id), n)
    } else {
        format!("/extensions/{}/logs", encode_segment(id))
    };
    let data = client.get(&path).await?;
    Ok(CliResponse::success(data, "Extension logs retrieved"))
}

/// Install extension from file
pub async fn install_extension_file(client: &ApiClient, file_path: &str) -> Result<CliResponse> {
    // Extension upload API expects JSON body with base64-encoded file data
    use std::io::Read;
    let mut file = std::fs::File::open(file_path)?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &buf);
    let filename = std::path::Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("extension.nep")
        .to_string();

    let body = json!({
        "data": b64,
        "filename": filename,
    });
    let data = client.post("/extensions/upload/file", &body).await?;

    Ok(CliResponse::success(data, "Extension installed"))
}

/// Install extension from marketplace
pub async fn install_extension_market(
    client: &ApiClient,
    extension_id: &str,
    version: Option<&str>,
) -> Result<CliResponse> {
    // Field is `id` — the handler's MarketplaceInstallRequest. This sent
    // `extension_id` since inception, so every CLI market-install failed
    // with "missing field `id`" before the request reached any install
    // logic (found 2026-10-15 verifying the chat-driven install path).
    let mut body = json!({
        "id": extension_id,
    });
    if let Some(v) = version {
        body["version"] = json!(v);
    }

    let data = client.post("/extensions/market/install", &body).await?;

    // The handler reports EVERY failure (unknown id, network, sha mismatch,
    // no build for the platform — 17 paths) as HTTP 200 whose INNER payload
    // carries success:false. The outer ApiResponse envelope says success:true
    // (the request was served), so the verdict must be read from the inner
    // body: `{"success":true,"data":{"success":false,"error":"…"}}`. Trusting
    // the status code wrapped those failures in a success envelope:
    // `market-install <bogus>` printed "✅ Extension installed" with exit 0
    // while nothing ran (2026-10-15 audit). Fall back to the flat shape for
    // unwrapped/older servers.
    let inner = data.get("data").unwrap_or(&data);
    if inner.get("success").and_then(|v| v.as_bool()) == Some(false) {
        let err = inner
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown marketplace failure");
        anyhow::bail!("marketplace install failed: {err}");
    }

    Ok(CliResponse::success(
        data,
        "Extension installed from marketplace",
    ))
}

/// Uninstall extension
pub async fn uninstall_extension(client: &ApiClient, id: &str) -> Result<CliResponse> {
    client
        .delete(&format!("/extensions/{}/uninstall", id))
        .await?;
    Ok(CliResponse::success(
        json!({ "id": id }),
        "Extension uninstalled",
    ))
}

/// Reload an extension (restart from file)
pub async fn reload_extension(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client
        .post_raw(&format!("/extensions/{}/reload", id))
        .await?;
    Ok(CliResponse::success(data, "Extension reloaded"))
}

/// List marketplace extensions
pub async fn list_marketplace(
    client: &ApiClient,
    filter: Option<&str>,
    verbose: bool,
) -> Result<CliResponse> {
    let data = client.get("/extensions/market/list").await?;
    if verbose {
        return Ok(CliResponse::success(data, "Marketplace extensions listed"));
    }
    // Compact by default: the raw registry is ~48 KB (26 extensions × full
    // metadata + URLs) and drowned small models' context when the agent ran
    // it — twice in one turn (2026-10-15). One line per extension is what
    // both a human and a 2B model actually need to pick an id to install;
    // `-v` keeps the full dump one flag away.
    let compact = compact_market_list(&data, filter);
    Ok(CliResponse::success(
        compact,
        "Marketplace extensions listed (compact; -v for full metadata)",
    ))
}

/// Project the raw market registry into {total, matched, extensions:[{id,
/// name, version, description}]}, optionally filtered by a case-insensitive
/// substring over id/name/description. Pure — unit-tested.
pub fn compact_market_list(raw: &serde_json::Value, filter: Option<&str>) -> serde_json::Value {
    // The registry nests: {success, data:{extensions:[...]}} — unwrap both.
    let list = raw
        .pointer("/data/extensions")
        .or_else(|| raw.pointer("/extensions"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let needle = filter.map(str::to_lowercase);
    let mut out = Vec::new();
    for e in &list {
        let desc = e
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or_default()
            .to_string();
        let entry = json!({
            "id": e.get("id").cloned().unwrap_or(serde_json::Value::Null),
            "name": e.get("name").cloned().unwrap_or(serde_json::Value::Null),
            "version": e.get("version").cloned().unwrap_or(serde_json::Value::Null),
            "description": desc,
        });
        if let Some(n) = &needle {
            let hay = format!(
                "{} {} {}",
                entry["id"].as_str().unwrap_or_default(),
                entry["name"].as_str().unwrap_or_default(),
                entry["description"].as_str().unwrap_or_default(),
            )
            .to_lowercase();
            if !hay.contains(n) {
                continue;
            }
        }
        out.push(entry);
    }
    json!({
        "total": list.len(),
        "matched": out.len(),
        "extensions": out,
    })
}

/// Search AI-visible extension commands by task text (NM-252).
///
/// Hits the host registry snapshot via
/// `GET /extensions/commands/search` — same visibility as invoke
/// (`ai_enabled` / `disabled_commands` / `ai_exposed`). Host ranks and
/// truncates; zero hits are a valid outcome.
pub async fn search_extension_commands(
    client: &ApiClient,
    query: &str,
    scope_extension_id: Option<&str>,
) -> Result<CliResponse> {
    let mut path = format!(
        "/extensions/commands/search?q={}",
        urlencoding_encode(query)
    );
    if let Some(id) = scope_extension_id {
        path.push_str(&format!("&extension={}", urlencoding_encode(id)));
    }
    let data = client.get(&path).await?;
    let inner = data.get("data").cloned().unwrap_or(data);
    let hits = inner
        .get("hits")
        .and_then(|h| h.as_array())
        .cloned()
        .unwrap_or_default();
    let n = hits.len();
    let needs_schema = hits.iter().any(|h| {
        h.get("requires_schema_lookup")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    });
    let mut resp = CliResponse::success(
        inner,
        if n == 0 {
            "No matching AI-visible extension commands".into()
        } else {
            format!("{n} matching command(s)")
        },
    );
    if n == 0 {
        resp = resp.with_suggestion(
            "No relevant extension command. Try a clearer task phrase, install the needed \
extension, or use web tools for announcements/general web intent.",
        );
    } else if needs_schema {
        resp = resp.with_suggestion(
            "Prefer hits[].shell_example `{command, params}` (replace <placeholders>). \
requires_schema_lookup → `neomind extension commands <id>` for full schema first. \
Reuse known id/command on follow-up.",
        );
    } else {
        resp = resp.with_suggestion(
            "Prefer hits[].shell_example `{command, params}` — paste into the shell tool \
(replace <placeholders>). Reuse known on follow-up; full schema via commands <id> only if needed.",
        );
    }
    Ok(resp)
}

/// Minimal query-string encoder (enough for search text / extension ids).
fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// List unified business command defs for an installed extension.
///
/// Includes schema, capability, AI visibility, and a concrete invoke example
/// so the model can go list → commands → invoke without re-scanning help.
pub async fn list_extension_commands(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client
        .get(&format!("/extensions/{}/commands", encode_segment(id)))
        .await?;
    // API may wrap as {success,data:[...]} or return the array directly.
    let commands = data
        .as_array()
        .cloned()
        .or_else(|| data.get("data").and_then(|d| d.as_array()).cloned())
        .unwrap_or_default();

    let summary: Vec<serde_json::Value> = commands
        .iter()
        .map(|c| {
            // API CommandDef uses `id` for the short command name.
            let name = c
                .get("id")
                .and_then(|v| v.as_str())
                .or_else(|| c.get("command").and_then(|v| v.as_str()))
                .unwrap_or("?");
            let disabled = c.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false);
            let ai_exposed = c
                .get("ai_exposed")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let schema = c.get("input_schema").cloned().unwrap_or(json!({}));
            // Prefer server-validated params_example; fall back to schema-derived.
            let example_val = c
                .get("params_example")
                .cloned()
                .filter(|v| v.is_object())
                .or_else(|| example_params_value_from_schema(&schema));
            let example_params = example_val
                .as_ref()
                .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "{}".into()))
                .unwrap_or_else(|| "{}".into());
            // Model-facing: prefer structured shell params form (NM-247).
            let invoke_example = if example_val.is_some() {
                format!(
                    "shell: command=\"neomind extension invoke {id} {name}\" params={example_params}"
                )
            } else {
                format!(
                    "shell: command=\"neomind extension invoke {id} {name}\" params={{}} \
                     (see input_schema — no reliable params_example)"
                )
            };
            let mut entry = json!({
                "id": c.get("command_id").cloned().unwrap_or_else(|| {
                    json!(format!(
                        "extension.type/{}/{name}",
                        neomind_core::extension::type_base_of(id)
                    ))
                }),
                "command": name,
                "contract_version": c.get("contract_version").cloned()
                    .unwrap_or(json!(neomind_core::extension::CONTRACT_VERSION)),
                "description": c.get("description").cloned().unwrap_or(json!("")),
                "input_schema": schema,
                "effect": c.get("effect").cloned().unwrap_or(json!("mutating")),
                "replay_safety": c.get("replay_safety").cloned().unwrap_or(json!("unsafe")),
                "execution": c.get("execution").cloned().unwrap_or(json!("immediate")),
                "capability_source": c.get("capability_source").cloned()
                    .unwrap_or(json!({"kind":"conservative_default"})),
                "ai_exposed": ai_exposed,
                "disabled": disabled,
                "ai_invokable": ai_exposed && !disabled,
                "invoke_example": invoke_example,
                "cli_invoke_example": format!(
                    "neomind extension invoke {id} {name} --params '{example_params}'"
                ),
            });
            if let Some(ex) = example_val {
                if let Some(obj) = entry.as_object_mut() {
                    obj.insert("params_example".into(), ex);
                }
            }
            entry
        })
        .collect();

    let invokable: Vec<&str> = summary
        .iter()
        .filter(|c| c.get("ai_invokable").and_then(|v| v.as_bool()) == Some(true))
        .filter_map(|c| c.get("command").and_then(|v| v.as_str()))
        .collect();

    let mut resp = CliResponse::success(
        json!({
            "extension_id": id,
            "total": summary.len(),
            "ai_invokable_count": invokable.len(),
            "commands": summary,
        }),
        format!("{} command(s) for extension {}", summary.len(), id),
    );
    if summary.is_empty() {
        resp = resp.with_suggestion(format!(
            "No commands registered for '{id}'. Confirm it is installed and running: neomind extension status {id}"
        ));
    } else if invokable.is_empty() {
        resp = resp.with_suggestion(
            "All commands are AI-disabled or author-hidden (ai_exposed=false). Re-enable in extension settings if AI should call them.",
        );
    } else {
        let first = invokable[0];
        resp = resp.with_suggestion(format!(
            "Next: neomind extension invoke {id} {first} --params '{{...}}'  (use input_schema / invoke_example on each command)"
        ));
    }
    Ok(resp)
}

/// Build a minimal example JSON object from a JSON Schema for invoke hints.
///
/// Empty string is never invented for required free-form strings (NM-247).
/// Prefer enum/default; omit unreliable keys (returns None when incomplete).
fn example_params_value_from_schema(schema: &serde_json::Value) -> Option<serde_json::Value> {
    let props = schema.get("properties")?.as_object()?;
    let required: std::collections::HashSet<&str> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .collect::<std::collections::HashSet<_>>()
        })
        .unwrap_or_default();
    let mut obj = serde_json::Map::new();
    for (key, prop) in props {
        if !required.is_empty() && !required.contains(key.as_str()) {
            continue;
        }
        let sample = if let Some(d) = prop.get("default") {
            d.clone()
        } else if let Some(en) = prop.get("enum").and_then(|e| e.as_array()) {
            en.first()?.clone()
        } else {
            match prop.get("type").and_then(|t| t.as_str()) {
                Some("string") => {
                    // Do not invent "" or non-empty literals.
                    continue;
                }
                Some("number") | Some("integer") => json!(0),
                Some("boolean") => json!(false),
                Some("array") => json!([]),
                Some("object") => json!({}),
                _ => continue,
            }
        };
        obj.insert(key.clone(), sample);
        if obj.len() >= 6 {
            break;
        }
    }
    for req in &required {
        if !obj.contains_key(*req) {
            return None;
        }
    }
    // Validate against the same schema the server uses.
    if neomind_core::extension::validate_and_normalize(schema, &Value::Object(obj.clone())).is_err()
    {
        return None;
    }
    Some(Value::Object(obj))
}

/// Invoke an extension command via the shared HTTP execute path.
///
/// Prefers `--params` / `--params-file` JSON so small models avoid shell
/// quoting. Params are validated server-side against the same schema that
/// `extension commands` returns.
pub async fn invoke_extension_command(
    client: &ApiClient,
    extension_id: &str,
    command: &str,
    params_json: Option<&str>,
    params_file: Option<&std::path::Path>,
    prepared_operation_id: Option<&str>,
    structured_params: Option<serde_json::Value>,
) -> Result<CliResponse> {
    // Prefer host-managed object (NM-247) — never re-stringify through --params.
    let params = if let Some(v) = structured_params {
        if !v.is_object() && !v.is_null() {
            anyhow::bail!("shell params must be a JSON object");
        }
        if v.is_null() {
            json!({})
        } else {
            v
        }
    } else {
        load_invoke_params(params_json, params_file)?
    };

    // Client-side required-field check against the live command schema when
    // reachable — fail fast before the round-trip when possible. Server still
    // validates (source of truth).
    if let Ok(listed) = client
        .get(&format!(
            "/extensions/{}/commands",
            encode_segment(extension_id)
        ))
        .await
    {
        let commands = listed
            .as_array()
            .cloned()
            .or_else(|| listed.get("data").and_then(|d| d.as_array()).cloned())
            .unwrap_or_default();
        if let Some(cmd) = commands
            .iter()
            .find(|c| c.get("id").and_then(|v| v.as_str()) == Some(command))
        {
            if let Some(schema) = cmd.get("input_schema") {
                // Same validate+normalize as the server; discard canonical
                // here — server re-prepares as source of truth (or consumes
                // the Chat-authorized PreparedInvoke when provided).
                if let Err(msg) = neomind_core::extension::validate_and_normalize(schema, &params) {
                    anyhow::bail!("{msg}");
                }
            }
        } else if !commands.is_empty() {
            let names: Vec<&str> = commands
                .iter()
                .filter_map(|c| c.get("id").and_then(|v| v.as_str()))
                .collect();
            anyhow::bail!(
                "command '{command}' not found on extension '{extension_id}'. Available: {}",
                names.join(", ")
            );
        }
    }

    let mut body = json!({
        "command": command,
        "params": params,
    });
    if let Some(op) = prepared_operation_id {
        if let Some(obj) = body.as_object_mut() {
            obj.insert(
                "prepared_operation_id".into(),
                serde_json::Value::String(op.to_string()),
            );
        }
    }
    let data = client
        .post(
            &format!("/extensions/{}/invoke", encode_segment(extension_id)),
            &body,
        )
        .await?;

    // Prefer the server's prepared envelope (operation_id, contract, …).
    let inner = data.get("data").cloned().unwrap_or(data);
    Ok(CliResponse::success(
        inner,
        format!("Invoked {extension_id}/{command}"),
    ))
}

fn load_invoke_params(
    params_json: Option<&str>,
    params_file: Option<&std::path::Path>,
) -> Result<serde_json::Value> {
    if let Some(path) = params_file {
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("failed to read --params-file {}: {e}", path.display()))?;
        let v = crate::json_arg::parse_json_arg(text.trim())
            .map_err(|e| anyhow::anyhow!("--params-file must contain a JSON object: {e}"))?;
        if !v.is_object() && !v.is_null() {
            anyhow::bail!("--params-file must contain a JSON object");
        }
        return Ok(if v.is_null() { json!({}) } else { v });
    }
    match params_json {
        None => Ok(json!({})),
        Some(s) => {
            let v = crate::json_arg::parse_json_arg(s.trim())
                .map_err(|e| anyhow::anyhow!("--params must be a JSON object: {e}"))?;
            if !v.is_object() && !v.is_null() {
                anyhow::bail!("--params must be a JSON object");
            }
            Ok(if v.is_null() { json!({}) } else { v })
        }
    }
}

/// Get extension configuration
pub async fn get_extension_config(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client.get(&format!("/extensions/{}/config", id)).await?;
    Ok(CliResponse::success(data, "Extension config retrieved"))
}

/// Update extension configuration
pub async fn update_extension_config(
    client: &ApiClient,
    id: &str,
    config: serde_json::Value,
) -> Result<CliResponse> {
    let data = client
        .put(&format!("/extensions/{}/config", id), &config)
        .await?;
    Ok(CliResponse::success(data, "Extension config updated"))
}

#[cfg(test)]
mod invoke_params_tests {
    use super::{example_params_value_from_schema, load_invoke_params};
    use serde_json::json;
    use std::io::Write;

    #[test]
    fn example_params_omit_unreliable_freeform_string() {
        let schema = json!({
            "type": "object",
            "required": ["city"],
            "properties": {
                "city": {"type": "string"},
                "lang": {"type": "string", "default": "en"},
                "units": {"type": "string"}
            }
        });
        // Required free-form string without enum/default → no invented "".
        assert!(example_params_value_from_schema(&schema).is_none());
    }

    #[test]
    fn example_params_prefer_enum_and_defaults() {
        let schema = json!({
            "type": "object",
            "required": ["city"],
            "properties": {
                "city": {"type": "string", "enum": ["Beijing", "Shanghai"]},
                "lang": {"type": "string", "default": "en"},
                "units": {"type": "string"}
            }
        });
        let v = example_params_value_from_schema(&schema).expect("reliable example");
        assert_eq!(v.get("city").and_then(|c| c.as_str()), Some("Beijing"));
        assert!(v.get("units").is_none(), "{v}");
    }

    #[test]
    fn default_empty_object() {
        assert_eq!(load_invoke_params(None, None).unwrap(), json!({}));
    }

    #[test]
    fn params_json_object() {
        assert_eq!(
            load_invoke_params(Some(r#"{"city":"Xiamen"}"#), None).unwrap(),
            json!({"city": "Xiamen"})
        );
    }

    #[test]
    fn params_file_wins_shape() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.json");
        std::fs::File::create(&path)
            .unwrap()
            .write_all(br#"{"city":"Shanghai"}"#)
            .unwrap();
        assert_eq!(
            load_invoke_params(None, Some(&path)).unwrap(),
            json!({"city": "Shanghai"})
        );
    }

    #[test]
    fn rejects_non_object() {
        assert!(load_invoke_params(Some("[1,2]"), None).is_err());
    }
}

#[cfg(test)]
mod market_list_tests {
    use super::compact_market_list;
    use serde_json::json;

    fn raw() -> serde_json::Value {
        json!({
            "success": true,
            "data": { "extensions": [
                { "id": "weather-forecast", "name": "Weather Forecast", "version": "2.14.0",
                  "description": "Real-time weather", "metadata_url": "https://long.url/..." },
                { "id": "yolo-video", "name": "YOLO Video", "version": "2.0.0",
                  "description": "Object detection", "metadata_url": "https://long.url/..." },
            ]}
        })
    }

    #[test]
    fn compact_keeps_one_line_fields_only() {
        let out = compact_market_list(&raw(), None);
        assert_eq!(out["total"], 2);
        assert_eq!(out["matched"], 2);
        let e = &out["extensions"][0];
        assert_eq!(e["id"], "weather-forecast");
        assert_eq!(e["version"], "2.14.0");
        assert!(
            e.get("metadata_url").is_none(),
            "URLs and bulk must be dropped"
        );
    }

    #[test]
    fn filter_matches_case_insensitively_across_fields() {
        // By id fragment, by name word, by description word.
        assert_eq!(compact_market_list(&raw(), Some("weather"))["matched"], 1);
        assert_eq!(compact_market_list(&raw(), Some("Forecast"))["matched"], 1);
        assert_eq!(compact_market_list(&raw(), Some("detection"))["matched"], 1);
        assert_eq!(compact_market_list(&raw(), Some("nomatch"))["matched"], 0);
    }

    #[test]
    fn tolerates_flat_shape_and_missing_fields() {
        let flat = json!({ "extensions": [ { "id": "x" } ] });
        let out = compact_market_list(&flat, None);
        assert_eq!(out["total"], 1);
        assert_eq!(out["extensions"][0]["id"], "x");
    }
}

#[cfg(test)]
mod market_install_verdict_tests {
    use serde_json::json;

    #[test]
    fn failure_envelope_is_recognized() {
        // The handler's failure shape: HTTP 200, success:false, error msg.
        // The verdict check mirrors the CLI fix — kept here as the shape's
        // executable documentation.
        // Real wire shape: ApiResponse envelope (success:true) around the
        // handler's own verdict (success:false).
        let body = json!({
            "success": true,
            "data": {
                "success": false,
                "extension_id": "bogus-ext-xyz",
                "downloaded": false,
                "installed": false,
                "error": "Extension not found: 404 Not Found"
            }
        });
        let inner = body.get("data").unwrap_or(&body);
        assert_eq!(
            inner["success"].as_bool(),
            Some(false),
            "inner verdict is the failure"
        );
        assert!(inner["error"].as_str().unwrap().contains("not found"));
    }
}
