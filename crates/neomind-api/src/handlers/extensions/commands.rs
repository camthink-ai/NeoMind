//! `commands` handlers — split from the former extensions.rs monolith.

use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};

use serde_json::json;

use crate::handlers::common::{ok, HandlerResult};
use crate::models::error::ErrorResponse;
use crate::server::ServerState;

use super::*;

/// Request to execute an extension command.
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct ExecuteCommandRequest {
    /// Command name
    pub command: String,
    /// Command arguments
    #[serde(default)]
    pub args: serde_json::Value,
}

/// POST /api/extensions/:id/command
/// Execute a command on an extension.
///
/// Includes panic protection to prevent server crashes from buggy extensions.
#[utoipa::path(
    post,
    path = "/api/extensions/{id}/command",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    request_body = ExecuteCommandRequest,
    responses(
        (status = 200, description = "Command dispatched; result payload returned"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn execute_extension_command_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<ExecuteCommandRequest>,
) -> HandlerResult<serde_json::Value> {
    let runtime = &state.extensions.runtime;

    // Check if extension exists first
    if !runtime.contains(&id).await {
        return Err(ErrorResponse::not_found(format!(
            "Extension '{}' not found",
            id
        )));
    }

    // Validate command name (non-empty, max 256 chars, no control characters)
    if req.command.is_empty() || req.command.len() > 256 {
        return Err(ErrorResponse::bad_request(
            "Command name must be 1-256 characters",
        ));
    }
    if req.command.chars().any(|c| c.is_control()) {
        return Err(ErrorResponse::bad_request(
            "Command name contains invalid characters",
        ));
    }

    // Validate args payload size (max 1 MB)
    if let Ok(size) = serde_json::to_string(&req.args).map(|s| s.len()) {
        if size > 1024 * 1024 {
            return Err(ErrorResponse::bad_request(
                "Command arguments too large (max 1 MB)",
            ));
        }
    }

    let prepared = prepare_extension_invoke(&state, &id, &req.command, &req.args).await?;

    // Execute with canonical params (same set used for auth/capability).
    let result = match runtime
        .execute_command(&id, &req.command, &prepared.canonical_params)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(
                extension_id = %id,
                command = %req.command,
                operation_id = %prepared.operation_id,
                error = %e,
                "Extension command execution failed"
            );
            return Err(ErrorResponse::internal(format!(
                "Command execution failed: {}",
                e
            )));
        }
    };

    // Publish ExtensionOutput events for agent triggers and dashboard subscriptions
    publish_extension_metrics_safe(&state, &id, &result).await;

    ok(json!({
        "operation_id": prepared.operation_id,
        "contract_version": prepared.contract.contract_version,
        "command_id": prepared.contract.id,
        "target": prepared.target,
        "effect": prepared.contract.effect,
        "replay_safety": prepared.contract.replay_safety,
        "capability_source": prepared.contract.capability_source,
        "param_fingerprint": prepared.param_fingerprint,
        "result": result,
    }))
}

/// Request to invoke an extension.
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct InvokeExtensionRequest {
    /// Command/function to invoke
    pub command: String,
    /// Input parameters
    #[serde(default)]
    pub params: serde_json::Value,
}

/// POST /api/extensions/:id/invoke
/// Invoke an extension and get JSON response.
///
/// This is a simplified version of execute_extension_command_handler
/// that returns results in a more JSON-friendly format for AI agents.
#[utoipa::path(
    post,
    path = "/api/extensions/{id}/invoke",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    request_body = InvokeExtensionRequest,
    responses(
        (status = 200, description = "Tool-style invocation of an extension entry point"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn invoke_extension_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(req): Json<InvokeExtensionRequest>,
) -> HandlerResult<serde_json::Value> {
    let runtime = &state.extensions.runtime;

    // Check if extension exists
    if !runtime.contains(&id).await {
        return Err(ErrorResponse::not_found(format!("Extension {}", id)));
    }

    // Validate command name
    if req.command.is_empty() || req.command.len() > 256 {
        return Err(ErrorResponse::bad_request(
            "Command name must be 1-256 characters",
        ));
    }
    if req.command.chars().any(|c| c.is_control()) {
        return Err(ErrorResponse::bad_request(
            "Command name contains invalid characters",
        ));
    }

    // Validate params payload size (max 1 MB)
    if let Ok(size) = serde_json::to_string(&req.params).map(|s| s.len()) {
        if size > 1024 * 1024 {
            return Err(ErrorResponse::bad_request(
                "Invoke parameters too large (max 1 MB)",
            ));
        }
    }

    let prepared = prepare_extension_invoke(&state, &id, &req.command, &req.params).await?;

    let result = match runtime
        .execute_command(&id, &req.command, &prepared.canonical_params)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(
                extension_id = %id,
                command = %req.command,
                operation_id = %prepared.operation_id,
                error = %e,
                "Extension invoke failed"
            );
            return Err(ErrorResponse::internal(format!("Invoke failed: {}", e)));
        }
    };

    publish_extension_metrics_safe(&state, &id, &result).await;

    ok(serde_json::json!({
        "extension_id": id,
        "command": req.command,
        "operation_id": prepared.operation_id,
        "contract_version": prepared.contract.contract_version,
        "command_id": prepared.contract.id,
        "target": prepared.target,
        "effect": prepared.contract.effect,
        "replay_safety": prepared.contract.replay_safety,
        "capability_source": prepared.contract.capability_source,
        "param_fingerprint": prepared.param_fingerprint,
        "result": result,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }))
}

/// Resolve persisted AI toggles from the **shared** extension store.
///
/// Fail closed:
/// - storage/`load` errors → [`ErrorResponse::service_unavailable`] (deny execute)
/// - missing record → forbidden (never default to allow)
/// - `uninstalled` → forbidden (never default to allow)
///
/// Scope note: HTTP `execute` / `invoke` both call this via
/// [`prepare_extension_invoke`], so the gate applies to **all** HTTP callers
/// including the human admin UI (intentional bypass prevention until a
/// trusted server-side caller identity exists — clients must not self-claim
/// "admin").
fn ai_policy_from_store(
    store: &neomind_storage::ExtensionStore,
    instance_id: &str,
) -> Result<(bool, Vec<String>), ErrorResponse> {
    let loaded = store.load(instance_id).map_err(|e| {
        ErrorResponse::service_unavailable(format!(
            "extension AI policy store unavailable for '{instance_id}': {e}"
        ))
    })?;
    ai_policy_from_loaded_record(instance_id, loaded)
}

/// Interpret an already-loaded policy record (or absence). Pure for tests.
fn ai_policy_from_loaded_record(
    instance_id: &str,
    loaded: Option<neomind_storage::ExtensionRecord>,
) -> Result<(bool, Vec<String>), ErrorResponse> {
    match loaded {
        None => Err(ErrorResponse::forbidden(format!(
            "no persisted AI policy record for extension '{instance_id}'; refuse invoke"
        ))),
        Some(r) if r.uninstalled => Err(ErrorResponse::forbidden(format!(
            "extension '{instance_id}' is marked uninstalled; refuse invoke"
        ))),
        Some(r) => Ok((r.enabled, r.disabled_commands)),
    }
}

/// Shared prepare for HTTP execute + invoke (same path as CLI / tools).
///
/// Enforces AI access policy (`enabled` / `disabled_commands` / `ai_exposed`)
/// at this boundary so Chat shell → `extension invoke` cannot bypass the
/// toggles that already hide direct tools from the LLM.
///
/// **Applies to every HTTP execute/invoke caller** (including human admin UI)
/// because there is no trusted server-side caller identity yet. Softening to
/// AI-only requires a non-client-claimable context, not a request flag.
async fn prepare_extension_invoke(
    state: &ServerState,
    instance_id: &str,
    command_name: &str,
    raw_params: &serde_json::Value,
) -> Result<neomind_core::extension::PreparedInvoke, ErrorResponse> {
    // Descriptor + generation from the proxy registry load snapshot (same
    // entry execute binds). Do not require isolated liveness here — the
    // outer handler already checked presence; policy/prepare must work from
    // the registered snapshot alone.
    let info = state
        .extensions
        .registry
        .get_info(instance_id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension '{instance_id}'")))?;

    let cmd = info
        .commands
        .iter()
        .find(|c| c.name == command_name)
        .ok_or_else(|| {
            let available: Vec<&str> = info.commands.iter().map(|c| c.name.as_str()).collect();
            ErrorResponse::bad_request(format!(
                "command '{command_name}' not found on extension '{instance_id}'. \
                 Available: [{}]. Next: neomind extension commands {instance_id}",
                available.join(", ")
            ))
        })?;

    // Shared pre-opened store — never reopen via global path (and never
    // default-allow on open/load failure).
    let (ext_ai_enabled, disabled_cmds) =
        ai_policy_from_store(state.extensions.store.as_ref(), instance_id)?;
    if let Some(reason) = neomind_core::extension::ai_command_access_denied(
        ext_ai_enabled,
        &disabled_cmds,
        command_name,
        cmd.ai_exposed,
    ) {
        return Err(ErrorResponse::forbidden(reason));
    }

    let version = Some(info.metadata.version.as_str());
    // Token comes from the same load snapshot as `cmd` / `info` — never "current".
    neomind_core::extension::prepare_invoke(neomind_core::extension::PrepareInvokeRequest {
        instance_id,
        command: cmd,
        raw_params,
        extension_version: version,
        package_digest: None,
        capability_generation: &info.capability_generation,
    })
    .map_err(ErrorResponse::bad_request)
}

/// GET /api/extensions/:id/stream
/// Get streaming output from an extension.
///
/// Returns information about the extension's stream capability.
/// For actual streaming, clients should use WebSocket or SSE endpoints.
///
/// V2: Streaming capability not declared in command config.
/// This endpoint always returns false for streaming support.
pub async fn stream_extension_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<serde_json::Value> {
    // Check if extension exists and get its info
    let info = state.extensions.runtime.get_info(&id).await;
    let Some(info) = info else {
        return Err(ErrorResponse::not_found(format!("Extension {}", id)));
    };

    // Isolated extensions support streaming via WebSocket
    let has_streaming = info.is_isolated && info.is_running;
    let stream_url = if has_streaming {
        format!("/api/extensions/{}/stream", id)
    } else {
        String::new()
    };

    ok(serde_json::json!({
        "extension_id": id,
        "supports_streaming": has_streaming,
        "stream_url": stream_url,
        "output_mode": if has_streaming { "stream" } else { "once" },
    }))
}

// ============================================================================
// Command-Based Extension DTOs and Handlers
// ============================================================================

/// Command descriptor DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandDescriptorDto {
    pub id: String,
    /// Type-level contract id: `extension.type/{type_base}/{command}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_id: Option<String>,
    /// Contract schema version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_version: Option<String>,
    /// Capability provenance after host resolve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_source: Option<neomind_core::extension::CapabilitySource>,
    pub display_name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub output_fields: Vec<OutputFieldDto>,
    pub config: CommandConfigDto,
    /// True when this command is excluded from the LLM tool registry — either
    /// named in the parent extension's `disabled_commands` list, the parent
    /// extension's master `enabled` flag is off, or the extension declared
    /// `ai_exposed=false` for it.
    #[serde(default)]
    pub disabled: bool,
    /// The extension author's declaration: false means UI/automation-only,
    /// never sent to the LLM. When false the toggle is locked.
    #[serde(default = "default_true")]
    pub ai_exposed: bool,
    /// Side-effect class after host resolve (`read_only` | `mutating`).
    #[serde(default)]
    pub effect: neomind_core::extension::CommandEffect,
    /// Replay safety after host resolve (`safe` | `unsafe`).
    #[serde(default)]
    pub replay_safety: neomind_core::extension::CommandReplaySafety,
    /// Immediate vs async track (`immediate` | `async`).
    #[serde(default)]
    pub execution: neomind_core::extension::CommandExecutionKind,
}

fn default_true() -> bool {
    true
}

/// Output field DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputFieldDto {
    pub name: String,
    pub data_type: String,
    pub unit: Option<String>,
    pub description: String,
    pub is_primary: bool,
    pub aggregatable: bool,
    pub default_agg_func: String,
}

/// Command configuration DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandConfigDto {
    pub requires_auth: bool,
    pub timeout_ms: u64,
    pub is_stream: bool,
    pub expected_duration_ms: Option<u64>,
}

/// Data source info DTO
/// Matches frontend ExtensionDataSourceInfo interface
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSourceInfoDto {
    pub id: String, // Format: "extension:{extension_id}:{command}:{field}"
    pub extension_id: String,
    pub command: String,
    pub field: String,
    pub display_name: String,
    pub data_type: String,
    pub unit: Option<String>,
    pub description: String,
    pub aggregatable: bool,
    pub default_agg_func: String,
}

/// GET /api/extensions/:id/commands
///
/// List all commands for an extension (V2 format)
#[utoipa::path(
    get,
    path = "/api/extensions/{id}/commands",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
    ),
    responses(
        (status = 200, description = "Toolbox commands exposed by an extension"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn list_extension_commands_handler(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> HandlerResult<Vec<CommandDescriptorDto>> {
    let info = state
        .extensions
        .runtime
        .get(&id)
        .await
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {}", id)))?;

    // Same fail-closed policy source as invoke/execute (shared store).
    let (ext_enabled, disabled_cmds_vec) =
        ai_policy_from_store(state.extensions.store.as_ref(), &id)?;
    let disabled_cmds: std::collections::HashSet<String> = disabled_cmds_vec.into_iter().collect();

    let version = Some(info.metadata.version.as_str());
    let result: Vec<CommandDescriptorDto> = info
        .commands
        .iter()
        .map(|cmd| {
            let def = neomind_core::extension::from_extension_command(&id, cmd, version, None);
            CommandDescriptorDto {
                id: cmd.name.clone(),
                command_id: Some(def.id.clone()),
                contract_version: Some(def.contract_version.clone()),
                capability_source: Some(def.capability_source.clone()),
                display_name: cmd.display_name.clone(),
                description: def.description.clone(),
                input_schema: def.input_schema.clone(),
                output_fields: vec![], // V2: Commands don't declare output fields
                config: CommandConfigDto {
                    requires_auth: false,
                    timeout_ms: 300000,
                    is_stream: false,
                    expected_duration_ms: None,
                },
                disabled: !ext_enabled || !cmd.ai_exposed || disabled_cmds.contains(&cmd.name),
                ai_exposed: cmd.ai_exposed,
                effect: def.effect,
                replay_safety: def.replay_safety,
                execution: def.execution,
            }
        })
        .collect();

    ok(result)
}

/// Request body for tool-enable toggles.
#[derive(utoipa::ToSchema, Debug, Deserialize)]
pub struct SetToolEnabledRequest {
    pub enabled: bool,
}

/// Rebuild the ToolRegistry disabled set from the on-disk state of all
/// extensions and push it live. Called from both PATCH handlers below so the
/// LLM picks up the change on the next tool-calling round without a server
/// restart. Cheap: O(n_extensions × n_commands) string formatting.
pub(crate) async fn refresh_tool_registry_disabled(state: &ServerState) {
    let Some(registry) = state.agents.session_manager.get_tool_registry().await else {
        return;
    };

    let mut disabled: std::collections::HashSet<String> = std::collections::HashSet::new();
    let store = state.extensions.store.clone();
    {
        if let Ok(records) = store.load_all() {
            for r in records {
                if !r.enabled {
                    // Master off: every command of this extension is hidden.
                    // We don't know all command names here cheaply, so we mark
                    // by prefix-lookup at the registry level using the disabled
                    // set as exact names; the registry's `definitions_for_llm`
                    // also needs a prefix mode. Simpler: walk the loaded
                    // extension runtime to enumerate command names.
                    if let Some(info) = state.extensions.runtime.get(&r.id).await {
                        for cmd in &info.commands {
                            disabled.insert(format!("{}:{}", r.id, cmd.name));
                        }
                    }
                    continue;
                }
                for cmd_name in &r.disabled_commands {
                    disabled.insert(format!("{}:{}", r.id, cmd_name));
                }
                // Author-declared hidden commands (ai_exposed=false) are
                // always hidden — admin toggles cannot override.
                if let Some(info) = state.extensions.runtime.get(&r.id).await {
                    for cmd in &info.commands {
                        if !cmd.ai_exposed {
                            disabled.insert(format!("{}:{}", r.id, cmd.name));
                        }
                    }
                }
            }
        }
    }
    registry.set_disabled(disabled);
}

/// PATCH /api/extensions/:id/commands/:cmd/enabled
///
/// Per-command tool-toggle. `enabled=false` adds the command to
/// `disabled_commands`; `enabled=true` removes it. Master `enabled` flag is
/// untouched. Storage is the source of truth; the live ToolRegistry is
/// refreshed from storage after the write.
#[utoipa::path(
    patch,
    path = "/api/extensions/{id}/commands/{cmd}/enabled",
    tag = "extensions",
    params(
        ("id" = String, Path, description = "Extension id"),
        ("cmd" = String, Path, description = "Command name"),
    ),
    request_body = SetToolEnabledRequest,
    responses(
        (status = 200, description = "Per-command enable flag updated"),
        (status = 404, description = "Not found"),
    )
)]
pub async fn set_extension_command_enabled_handler(
    State(state): State<ServerState>,
    Path((id, cmd)): Path<(String, String)>,
    Json(req): Json<SetToolEnabledRequest>,
) -> HandlerResult<serde_json::Value> {
    validate_extension_id(&id)?;
    if cmd.is_empty() {
        return Err(ErrorResponse::bad_request(
            "Command name required".to_string(),
        ));
    }

    // Shared pre-opened store from ServerState (was: per-request
    // ExtensionStore::open, whose failure swallowed into a 500).
    let store = state.extensions.store.clone();
    let mut record = store
        .load(&id)
        .map_err(|e| ErrorResponse::internal(format!("Load extension: {e}")))?
        .ok_or_else(|| ErrorResponse::not_found(format!("Extension {id}")))?;

    if req.enabled {
        record.disabled_commands.retain(|c| c != &cmd);
    } else if !record.disabled_commands.iter().any(|c| c == &cmd) {
        record.disabled_commands.push(cmd.clone());
    }
    record.touch();
    store
        .save(&record)
        .map_err(|e| ErrorResponse::internal(format!("Save extension: {e}")))?;

    refresh_tool_registry_disabled(&state).await;

    tracing::info!(
        extension = %id,
        command = %cmd,
        enabled = req.enabled,
        "extension command tool-toggle updated"
    );
    ok(json!({ "id": id, "command": cmd, "enabled": req.enabled }))
}

#[cfg(test)]
mod ai_policy_fail_closed_tests {
    use super::*;
    use axum::http::StatusCode;
    use neomind_core::extension::system::{
        DynExtension, Extension, ExtensionCommand, ExtensionMetadata, MetricDefinition,
    };
    use neomind_storage::ExtensionRecord;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, OnceLock};

    fn ping_cmd() -> ExtensionCommand {
        ExtensionCommand {
            name: "ping".into(),
            display_name: "Ping".into(),
            description: "test".into(),
            payload_template: String::new(),
            parameters: Vec::new(),
            fixed_values: std::collections::HashMap::new(),
            samples: Vec::new(),
            parameter_groups: Vec::new(),
            ai_exposed: true,
            effect: Some(neomind_core::extension::system::CommandEffect::ReadOnly),
            replay_safety: Some(neomind_core::extension::system::CommandReplaySafety::Safe),
            execution: Some(neomind_core::extension::system::CommandExecutionKind::Immediate),
        }
    }

    struct CountingExt {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl Extension for CountingExt {
        fn metadata(&self) -> &ExtensionMetadata {
            static M: OnceLock<ExtensionMetadata> = OnceLock::new();
            M.get_or_init(|| ExtensionMetadata::new("ai-policy-ext", "AI Policy Ext", "1.0.0"))
        }
        fn metrics(&self) -> Vec<MetricDefinition> {
            Vec::new()
        }
        fn commands(&self) -> Vec<ExtensionCommand> {
            vec![ping_cmd()]
        }
        async fn execute_command(
            &self,
            _command: &str,
            _args: &serde_json::Value,
        ) -> neomind_core::extension::system::Result<serde_json::Value> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"ok": true}))
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    fn base_record(id: &str) -> ExtensionRecord {
        ExtensionRecord::new(
            id.into(),
            "AI Policy Ext".into(),
            "/tmp/ai-policy-ext".into(),
            "native".into(),
            "1.0.0".into(),
        )
    }

    /// Mirror HTTP invoke/execute: prepare (AI gate) then registry execute.
    /// Counts prove a failed gate never reaches the extension business fn.
    async fn http_prepare_then_registry_execute(
        state: &ServerState,
        id: &str,
        command: &str,
        calls: &AtomicUsize,
    ) -> Result<serde_json::Value, ErrorResponse> {
        let before = calls.load(Ordering::SeqCst);
        let prepared = prepare_extension_invoke(state, id, command, &json!({})).await?;
        let out = state
            .extensions
            .registry
            .execute_command(id, command, &prepared.canonical_params)
            .await
            .map_err(|e| ErrorResponse::internal(e.to_string()))?;
        assert!(
            calls.load(Ordering::SeqCst) > before,
            "successful prepare must reach extension execute"
        );
        Ok(out)
    }

    #[test]
    fn loaded_record_none_and_uninstalled_never_default_allow() {
        let err = ai_policy_from_loaded_record("x", None).unwrap_err();
        assert_eq!(err.status, StatusCode::FORBIDDEN);
        assert!(err.message.contains("no persisted AI policy"));

        let mut r = base_record("x");
        r.uninstalled = true;
        let err = ai_policy_from_loaded_record("x", Some(r)).unwrap_err();
        assert_eq!(err.status, StatusCode::FORBIDDEN);
        assert!(err.message.contains("uninstalled"));
    }

    #[test]
    fn store_load_error_is_service_unavailable() {
        let store = neomind_storage::ExtensionStore::open(":memory:").unwrap();
        store
            .insert_unreadable_bytes_for_test("broken", b"{not-json")
            .unwrap();
        let err = ai_policy_from_store(store.as_ref(), "broken").unwrap_err();
        assert_eq!(err.status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(err.message.contains("policy store unavailable"));
    }

    #[tokio::test]
    async fn persisted_disable_blocks_http_prepare_before_execute() {
        let state = ServerState::new_for_testing().await;
        let id = "ai-policy-ext";
        let calls = Arc::new(AtomicUsize::new(0));
        let ext: DynExtension = Arc::new(tokio::sync::RwLock::new(Box::new(CountingExt {
            calls: calls.clone(),
        })));
        state
            .extensions
            .registry
            .register(id.into(), ext)
            .await
            .unwrap();

        let mut rec = base_record(id);
        rec.disabled_commands = vec!["ping".into()];
        state.extensions.store.save(&rec).unwrap();

        let err = http_prepare_then_registry_execute(&state, id, "ping", calls.as_ref())
            .await
            .unwrap_err();
        assert_eq!(err.status, StatusCode::FORBIDDEN, "msg={}", err.message);
        assert!(
            err.message.contains("disabled_commands") || err.message.contains("disabled"),
            "got: {}",
            err.message
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "disabled command must not reach extension execute"
        );
    }

    #[tokio::test]
    async fn store_read_failure_after_disable_never_reaches_execute() {
        let state = ServerState::new_for_testing().await;
        let id = "ai-policy-readfail";
        let calls = Arc::new(AtomicUsize::new(0));

        // Distinct metadata id must match register id for snapshot lookup.
        struct CountingExt2 {
            id: String,
            calls: Arc<AtomicUsize>,
        }
        #[async_trait::async_trait]
        impl Extension for CountingExt2 {
            fn metadata(&self) -> &ExtensionMetadata {
                // Leak one metadata per test id — fine in unit tests.
                Box::leak(Box::new(ExtensionMetadata::new(
                    self.id.clone(),
                    "RF",
                    "1.0.0",
                )))
            }
            fn metrics(&self) -> Vec<MetricDefinition> {
                Vec::new()
            }
            fn commands(&self) -> Vec<ExtensionCommand> {
                vec![ping_cmd()]
            }
            async fn execute_command(
                &self,
                _command: &str,
                _args: &serde_json::Value,
            ) -> neomind_core::extension::system::Result<serde_json::Value> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(json!({"ok": true}))
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        let ext: DynExtension = Arc::new(tokio::sync::RwLock::new(Box::new(CountingExt2 {
            id: id.into(),
            calls: calls.clone(),
        })));
        state
            .extensions
            .registry
            .register(id.into(), ext)
            .await
            .unwrap();

        // Persist a real disable first (policy intent), then corrupt the row so
        // the next load errors — must stay deny, never fall open to execute.
        let mut rec = base_record(id);
        rec.disabled_commands = vec!["ping".into()];
        state.extensions.store.save(&rec).unwrap();
        state
            .extensions
            .store
            .insert_unreadable_bytes_for_test(id, b"{\"enabled\":true,\"disabled_commands\":[]")
            .unwrap();

        let err = http_prepare_then_registry_execute(&state, id, "ping", calls.as_ref())
            .await
            .unwrap_err();
        assert_eq!(
            err.status,
            StatusCode::SERVICE_UNAVAILABLE,
            "msg={}",
            err.message
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "store read failure must not reach extension execute"
        );
    }

    #[tokio::test]
    async fn enabled_policy_allows_prepare_and_execute() {
        let state = ServerState::new_for_testing().await;
        let id = "ai-policy-ok";
        let calls = Arc::new(AtomicUsize::new(0));

        struct CountingExt3 {
            calls: Arc<AtomicUsize>,
        }
        #[async_trait::async_trait]
        impl Extension for CountingExt3 {
            fn metadata(&self) -> &ExtensionMetadata {
                static M: OnceLock<ExtensionMetadata> = OnceLock::new();
                M.get_or_init(|| ExtensionMetadata::new("ai-policy-ok", "OK", "1.0.0"))
            }
            fn metrics(&self) -> Vec<MetricDefinition> {
                Vec::new()
            }
            fn commands(&self) -> Vec<ExtensionCommand> {
                vec![ping_cmd()]
            }
            async fn execute_command(
                &self,
                _command: &str,
                _args: &serde_json::Value,
            ) -> neomind_core::extension::system::Result<serde_json::Value> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(json!({"ok": true}))
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        let ext: DynExtension = Arc::new(tokio::sync::RwLock::new(Box::new(CountingExt3 {
            calls: calls.clone(),
        })));
        state
            .extensions
            .registry
            .register(id.into(), ext)
            .await
            .unwrap();
        state.extensions.store.save(&base_record(id)).unwrap();

        http_prepare_then_registry_execute(&state, id, "ping", calls.as_ref())
            .await
            .expect("enabled policy must allow");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
