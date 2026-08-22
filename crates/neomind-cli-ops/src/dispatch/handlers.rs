//! Data-command handlers for in-process dispatch.
//!
//! Each handler returns `(CliResponse, OutputFormat)` instead of printing
//! directly, so the same logic serves both the real binary (which prints via
//! `format_output`) and the in-process dispatcher (which returns the data to
//! the agent without touching stdout).

use crate::types::{CliResponse, OutputFormat};
use anyhow::Result;
// Bring all clap command types into scope.
#[allow(unused_imports)]
use super::commands::*;

/// Returns true for extension subcommands that touch the local filesystem
/// (`validate`, `install`, `uninstall`, `create`, `build`, `info`) and must
/// run as a subprocess so their stdout is captured by the agent.
pub fn is_local_extension_command(cmd: &ExtensionCommand) -> bool {
    matches!(
        cmd,
        ExtensionCommand::Validate { .. }
            | ExtensionCommand::Install { .. }
            | ExtensionCommand::Uninstall { .. }
            | ExtensionCommand::Create { .. }
            | ExtensionCommand::Build { .. }
            | ExtensionCommand::Get { .. }
    )
}

pub async fn run_extension_cmd(cmd: ExtensionCommand) -> Result<(CliResponse, OutputFormat)> {
    // Local-only subcommands (validate/install/uninstall/create/build/info) are
    // handled by the binary directly — they print to stdout and rely on
    // subprocess capture. The dispatcher pre-filters them via
    // `is_local_extension_command`, so reaching this handler with a local
    // command is a programming error.
    if is_local_extension_command(&cmd) {
        anyhow::bail!(
            "local extension subcommands are not handled in-process; they must run as a subprocess"
        );
    }

    let api_base = std::env::var("NEOMIND_API_BASE")
        .unwrap_or_else(|_| "http://localhost:9375/api".to_string());
    let client = crate::ApiClient::with_base_url(&api_base);
    let output_format = if std::env::var("NEOMIND_JSON").is_ok() {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    };

    let response = match cmd {
        ExtensionCommand::List { verbose: _ } => crate::extension::list_extensions(&client).await?,
        ExtensionCommand::Status { id } => {
            crate::extension::get_extension_status(&client, &id).await?
        }
        ExtensionCommand::Logs { id, lines } => {
            crate::extension::get_extension_logs(&client, &id, lines).await?
        }
        ExtensionCommand::MarketInstall {
            extension_id,
            version,
        } => {
            crate::extension::install_extension_market(&client, &extension_id, version.as_deref())
                .await?
        }
        ExtensionCommand::MarketList => crate::extension::list_marketplace(&client).await?,
        ExtensionCommand::Reload { id } => crate::extension::reload_extension(&client, &id).await?,
        ExtensionCommand::Config { id, set } => match set {
            Some(json_str) => {
                let config = serde_json::from_str(&json_str).unwrap_or(serde_json::json!(json_str));
                crate::extension::update_extension_config(&client, &id, config).await?
            }
            None => crate::extension::get_extension_config(&client, &id).await?,
        },
        // Local commands are guarded above; any other variant is a bug.
        _ => unreachable!("unhandled extension subcommand reached run_extension_cmd"),
    };

    Ok((response, output_format))
}

pub async fn run_llm_cmd(cmd: LlmCommand) -> Result<(CliResponse, OutputFormat)> {
    use crate::llm::*;

    let api_base = std::env::var("NEOMIND_API_BASE")
        .unwrap_or_else(|_| "http://localhost:9375/api".to_string());
    let client = crate::ApiClient::with_base_url(&api_base);
    let output_format = if std::env::var("NEOMIND_JSON").is_ok() {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    };

    let response = match cmd {
        LlmCommand::List {} => list_backends(&client).await?,
        LlmCommand::Get { id } => get_backend(&client, &id).await?,
        LlmCommand::Models { endpoint: _ } => list_ollama_models(&client).await?,
        LlmCommand::Create {
            name,
            r#type,
            endpoint,
            model,
            api_key,
            temperature,
        } => {
            create_backend(
                &client,
                &name,
                &r#type,
                &endpoint,
                &model,
                api_key.as_deref(),
                temperature,
            )
            .await?
        }
        LlmCommand::Update {
            id,
            name,
            model,
            endpoint,
            api_key,
            temperature,
        } => {
            update_backend(
                &client,
                &id,
                name.as_deref(),
                model.as_deref(),
                endpoint.as_deref(),
                api_key.as_deref(),
                temperature,
            )
            .await?
        }
        LlmCommand::Delete { id } => delete_backend(&client, &id).await?,
        LlmCommand::Activate { id } => activate_backend(&client, &id).await?,
        LlmCommand::Test { id } => test_backend(&client, &id).await?,
    };

    Ok((response, output_format))
}

pub async fn run_device_cmd(cmd: DeviceCommand) -> Result<(CliResponse, OutputFormat)> {
    use crate::{device::*, ApiClient};

    // Get API base URL from environment or use default
    let api_base = std::env::var("NEOMIND_API_BASE")
        .unwrap_or_else(|_| "http://localhost:9375/api".to_string());

    // Create API client
    let client = ApiClient::with_base_url(&api_base);

    // Resolve output format: NEOMIND_JSON env var > Human default
    let base_format = if std::env::var("NEOMIND_JSON").is_ok() {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    };

    let (response, output_format) = match cmd {
        DeviceCommand::List {
            device_type,
            status,
        } => (
            list_devices(&client, device_type.as_deref(), status.as_deref()).await?,
            base_format,
        ),
        DeviceCommand::Get { id, metric } => (
            get_device(&client, &id, metric.as_deref()).await?,
            base_format,
        ),
        DeviceCommand::Create {
            name,
            device_type,
            adapter_type,
            device_id,
            config,
        } => {
            let connection_config = if let Some(config_str) = config {
                Some(serde_json::from_str(&config_str)?)
            } else {
                None
            };
            (
                create_device(
                    &client,
                    &name,
                    &device_type,
                    &adapter_type,
                    device_id.as_deref(),
                    connection_config,
                )
                .await?,
                base_format,
            )
        }
        DeviceCommand::Update { id, name, config } => {
            let connection_config = if let Some(config_str) = config {
                Some(serde_json::from_str(&config_str)?)
            } else {
                None
            };
            (
                update_device(&client, &id, name.as_deref(), connection_config).await?,
                base_format,
            )
        }
        DeviceCommand::Delete { id } => (delete_device(&client, &id).await?, base_format),
        DeviceCommand::Latest { id } => (get_device(&client, &id, None).await?, base_format),
        DeviceCommand::History {
            id,
            metric,
            time_range,
            compress,
            limit,
        } => (
            get_telemetry_history(
                &client,
                &id,
                metric.as_deref(),
                time_range.as_deref(),
                compress.unwrap_or(false),
                limit,
            )
            .await?,
            base_format,
        ),
        DeviceCommand::Control {
            id,
            command,
            params,
        } => {
            let params_json = if let Some(params_str) = params {
                serde_json::from_str(&params_str)?
            } else {
                serde_json::json!({})
            };
            (
                control_device(&client, &id, &command, params_json).await?,
                base_format,
            )
        }
        DeviceCommand::Types { type_cmd } => {
            return run_device_type_cmd(client, type_cmd, base_format).await;
        }
        DeviceCommand::WriteMetric {
            id,
            metric,
            value,
            timestamp,
        } => {
            // Try parsing value as number, bool, then fallback to string
            let value_json = if let Ok(n) = value.parse::<f64>() {
                serde_json::json!(n)
            } else if let Ok(b) = value.parse::<bool>() {
                serde_json::json!(b)
            } else {
                serde_json::json!(value)
            };
            (
                write_metric(&client, &id, &metric, value_json, timestamp).await?,
                base_format,
            )
        }
        DeviceCommand::WebhookUrl { id } => (get_webhook_url(&client, &id).await?, base_format),
        DeviceCommand::Drafts { draft_cmd } => {
            return run_draft_cmd(draft_cmd).await;
        }
    };

    // Format and print output
    Ok((response, output_format))
}

/// Run device draft management commands.
pub async fn run_draft_cmd(cmd: DraftCommand) -> Result<(CliResponse, OutputFormat)> {
    use crate::device::*;

    let client = crate::ApiClient::new();
    let base_format = if std::env::var("NEOMIND_JSON").is_ok() {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    };
    let response = match cmd {
        DraftCommand::List {} => (list_drafts(&client).await?, base_format),
        DraftCommand::Get { id } => (get_draft(&client, &id).await?, base_format),
        DraftCommand::Approve { id, name, r#type } => (
            approve_draft(&client, &id, name.as_deref(), r#type.as_deref()).await?,
            base_format,
        ),
        DraftCommand::Reject { id } => (reject_draft(&client, &id).await?, base_format),
        DraftCommand::Config {
            enabled,
            auto_approve,
            max_samples,
        } => {
            if enabled.is_some() || auto_approve.is_some() || max_samples.is_some() {
                (
                    update_onboard_config(&client, enabled, max_samples, auto_approve).await?,
                    base_format,
                )
            } else {
                (get_onboard_config(&client).await?, base_format)
            }
        }
    };

    Ok((response.0, response.1))
}

/// Run device type management commands.
pub async fn run_device_type_cmd(
    client: crate::ApiClient,
    cmd: DeviceTypeCommand,
    output_format: crate::types::OutputFormat,
) -> Result<(CliResponse, OutputFormat)> {
    use crate::device::*;

    let response = match cmd {
        DeviceTypeCommand::List => list_device_types(&client).await?,
        DeviceTypeCommand::Get { id } => get_device_type(&client, &id).await?,
        DeviceTypeCommand::Create {
            id,
            name,
            metrics,
            commands,
        } => {
            let metrics_json = serde_json::from_str(&metrics)?;
            let commands_json = if let Some(cmds_str) = commands {
                Some(serde_json::from_str(&cmds_str)?)
            } else {
                None
            };
            create_device_type(&client, id.as_deref(), &name, metrics_json, commands_json).await?
        }
        DeviceTypeCommand::Delete { id } => delete_device_type(&client, &id).await?,
    };

    // Format and print output
    Ok((response, output_format))
}

/// Run dashboard management commands.
pub async fn run_dashboard_cmd(cmd: DashboardCommand) -> Result<(CliResponse, OutputFormat)> {
    use crate::{dashboard::*, ApiClient};

    // Get API base URL from environment or use default
    let api_base = std::env::var("NEOMIND_API_BASE")
        .unwrap_or_else(|_| "http://localhost:9375/api".to_string());

    // Create API client
    let client = ApiClient::with_base_url(&api_base);

    // Get output format (controlled by NEOMIND_JSON env var)
    let output_format = if std::env::var("NEOMIND_JSON").is_ok() {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    };

    let (response, output_format) = match cmd {
        DashboardCommand::List {} => {
            let resp = list_dashboards(&client).await?;
            (resp, output_format)
        }
        DashboardCommand::Get { id } => {
            let resp = get_dashboard(&client, &id).await?;
            (resp, output_format)
        }
        DashboardCommand::Create {
            name,
            description,
            layout,
            components,
        } => {
            let layout_json = if let Some(layout_str) = layout {
                Some(serde_json::from_str(&layout_str)?)
            } else {
                None
            };
            let components_json = if let Some(components_str) = components {
                Some(serde_json::from_str(&components_str)?)
            } else {
                None
            };
            let resp =
                create_dashboard(&client, &name, description.as_deref(), layout_json, components_json).await?;
            (resp, output_format)
        }
        DashboardCommand::Update {
            id,
            name,
            description,
            layout,
            components,
            replace_all,
        } => {
            // Explicit gate: --components replaces the ENTIRE component array.
            // Small models repeatedly reach for it when they mean "add" or
            // "tweak one widget" — without this gate those mistakes silently
            // wipe the dashboard. Fail loudly with the right command instead.
            if components.is_some() && !replace_all {
                return Err(anyhow::anyhow!(
                    "--components replaces ALL dashboard components and needs an explicit --replace-all confirmation.\n\
                     Most likely you want one of these instead:\n\
                     - ADD widgets:    neomind dashboard add-components {id} --components '[...]'\n\
                     - TWEAK one widget: neomind dashboard update-component {id} --component-id <cid> --set '{...}'\n\
                     - TRULY replace everything: re-run with --replace-all"
                        .replace("{id}", &id)
                ));
            }
            let layout_json = if let Some(layout_str) = layout {
                Some(serde_json::from_str(&layout_str)?)
            } else {
                None
            };
            let components_json = if let Some(components_str) = components {
                Some(serde_json::from_str(&components_str)?)
            } else {
                None
            };
            let resp = update_dashboard(
                &client,
                &id,
                name.as_deref(),
                description.as_deref(),
                layout_json,
                components_json,
            )
            .await?;
            (resp, output_format)
        }
        DashboardCommand::Delete { id } => (delete_dashboard(&client, &id).await?, output_format),
        DashboardCommand::AddComponents { id, components } => {
            // Propagate parse errors — a silent empty-array fallback made
            // malformed JSON "succeed" with zero components added, and the
            // model retried in a confused loop (observed ×5 in evals).
            let comps = serde_json::from_str(&components)?;
            let resp = add_components(&client, &id, comps).await?;
            (resp, output_format)
        }
        DashboardCommand::UpdateComponent {
            id,
            component_id,
            set,
        } => {
            let patch = serde_json::from_str(&set)?;
            let resp = update_component(&client, &id, &component_id, patch).await?;
            (resp, output_format)
        }
        DashboardCommand::RemoveComponents { id, ids } => {
            let ids_val = serde_json::from_str(&ids)?;
            let resp = remove_components(&client, &id, ids_val).await?;
            (resp, output_format)
        }
        DashboardCommand::Share {
            id,
            public,
            expires,
        } => {
            let resp =
                share_dashboard(&client, &id, public.unwrap_or(false), expires.as_deref()).await?;
            (resp, output_format)
        }
    };

    // Format and print output
    Ok((response, output_format))
}

/// Run rule management commands.
pub async fn run_rule_cmd(cmd: RuleCommand) -> Result<(CliResponse, OutputFormat)> {
    use crate::{rule::*, ApiClient};

    // Get API base URL from environment or use default
    let api_base = std::env::var("NEOMIND_API_BASE")
        .unwrap_or_else(|_| "http://localhost:9375/api".to_string());

    // Create API client
    let client = ApiClient::with_base_url(&api_base);

    // Get output format (controlled by NEOMIND_JSON env var)
    let output_format = if std::env::var("NEOMIND_JSON").is_ok() {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    };

    let response = match cmd {
        RuleCommand::List => list_rules(&client).await?,
        RuleCommand::Get { id } => get_rule(&client, &id).await?,
        RuleCommand::Create { body } => create_rule(&client, &body).await?,
        RuleCommand::Update { id, body } => update_rule(&client, &id, &body).await?,
        RuleCommand::Delete { id } => delete_rule(&client, &id).await?,
        RuleCommand::Enable { id } => enable_rule(&client, &id).await?,
        RuleCommand::Disable { id } => disable_rule(&client, &id).await?,
        RuleCommand::Test { id, input } => {
            let input_json = serde_json::from_str(&input)?;
            test_rule(&client, &id, input_json).await?
        }
        RuleCommand::History { id } => get_rule_history(&client, &id).await?,
    };

    // Format and print output
    Ok((response, output_format))
}

/// Run transform management commands.
pub async fn run_transform_cmd(cmd: TransformCommand) -> Result<(CliResponse, OutputFormat)> {
    use crate::{transform::*, ApiClient};

    // Get API base URL from environment or use default
    let api_base = std::env::var("NEOMIND_API_BASE")
        .unwrap_or_else(|_| "http://localhost:9375/api".to_string());

    // Create API client
    let client = ApiClient::with_base_url(&api_base);

    // Get output format (controlled by NEOMIND_JSON env var)
    let output_format = if std::env::var("NEOMIND_JSON").is_ok() {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    };

    let response = match cmd {
        TransformCommand::List => list_transforms(&client).await?,
        TransformCommand::Get { id } => get_transform(&client, &id).await?,
        TransformCommand::Create {
            name,
            scope,
            code,
            output_prefix,
            description,
            enabled,
        } => {
            create_transform(
                &client,
                &name,
                &scope,
                &code,
                output_prefix.as_deref(),
                description.as_deref(),
                enabled,
            )
            .await?
        }
        TransformCommand::Update {
            id,
            name,
            description,
            code,
            scope,
            output_prefix,
            enabled,
        } => {
            update_transform(
                &client,
                &id,
                name.as_deref(),
                description.as_deref(),
                code.as_deref(),
                scope.as_deref(),
                output_prefix.as_deref(),
                enabled,
            )
            .await?
        }
        TransformCommand::Delete { id } => delete_transform(&client, &id).await?,
        TransformCommand::Enable { id } => enable_transform(&client, &id).await?,
        TransformCommand::Disable { id } => disable_transform(&client, &id).await?,
        TransformCommand::Metrics => list_virtual_metrics(&client).await?,
        TransformCommand::TestCode { code, input } => {
            let input_json = serde_json::from_str(&input)?;
            test_transform_code(&client, &code, input_json).await?
        }
        TransformCommand::DataSources => list_transform_data_sources(&client).await?,
    };

    // Format and print output
    Ok((response, output_format))
}

/// Run agent management commands.
pub async fn run_agent_cmd(cmd: AgentCommand) -> Result<(CliResponse, OutputFormat)> {
    use crate::{agent_cmd::*, ApiClient};

    // Get API base URL from environment or use default
    let api_base = std::env::var("NEOMIND_API_BASE")
        .unwrap_or_else(|_| "http://localhost:9375/api".to_string());

    // Create API client
    let client = ApiClient::with_base_url(&api_base);

    // Get output format (controlled by NEOMIND_JSON env var)
    let output_format = if std::env::var("NEOMIND_JSON").is_ok() {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    };

    let response = match cmd {
        AgentCommand::List => list_agents(&client).await?,
        AgentCommand::Get { id } => get_agent(&client, &id).await?,
        AgentCommand::Create {
            name,
            prompt,
            description,
            schedule_type,
            schedule_config,
            every,
            event_filter,
            timezone,
            llm_backend,
            system_prompt,
            execution_mode,
            device_ids,
            resources,
            metrics,
            commands,
            enable_tool_chaining,
            max_chain_depth,
            priority,
            context_window_size,
        } => {
            // Handle --every shortcut: parse duration to interval schedule
            let (resolved_st, resolved_sc) = if let Some(dur) = &every {
                let secs = parse_duration(dur);
                (Some("interval".to_string()), Some(secs.to_string()))
            } else {
                (schedule_type.clone(), schedule_config.clone())
            };
            create_agent(
                &client,
                &name,
                &prompt,
                description.as_deref(),
                resolved_st.as_deref(),
                resolved_sc.as_deref(),
                event_filter.as_deref(),
                timezone.as_deref(),
                llm_backend.as_deref(),
                system_prompt.as_deref(),
                execution_mode.as_deref(),
                device_ids.as_deref(),
                resources.as_deref(),
                metrics.as_deref(),
                commands.as_deref(),
                enable_tool_chaining,
                max_chain_depth,
                priority,
                context_window_size,
            )
            .await?
        }
        AgentCommand::Update {
            id,
            name,
            prompt,
            description,
            llm_backend,
            system_prompt,
            schedule_type,
            schedule_config,
            execution_mode,
            device_ids,
            resources,
            metrics,
            commands,
            enable_tool_chaining,
            max_chain_depth,
            priority,
            context_window_size,
        } => {
            update_agent(
                &client,
                &id,
                name.as_deref(),
                description.as_deref(),
                llm_backend.as_deref(),
                system_prompt.as_deref(),
                prompt.as_deref(),
                schedule_type.as_deref(),
                schedule_config.as_deref(),
                execution_mode.as_deref(),
                device_ids.as_deref(),
                resources.as_deref(),
                metrics.as_deref(),
                commands.as_deref(),
                enable_tool_chaining,
                max_chain_depth,
                priority,
                context_window_size,
            )
            .await?
        }
        AgentCommand::Delete { id } => delete_agent(&client, &id).await?,
        AgentCommand::Control { id, status } => control_agent(&client, &id, &status).await?,
        AgentCommand::Invoke { id, input } => invoke_agent(&client, &id, &input).await?,
        AgentCommand::Memory { id } => get_agent_memory(&client, &id).await?,
        AgentCommand::ClearMemory { id } => clear_agent_memory(&client, &id).await?,
        AgentCommand::Executions { id, limit, offset } => {
            get_agent_executions(&client, &id, limit, offset).await?
        }
        AgentCommand::LatestExecution { id } => get_latest_execution(&client, &id).await?,
        AgentCommand::Conversation { id, limit } => get_conversation(&client, &id, limit).await?,
        AgentCommand::SendMessage {
            id,
            message,
            message_type,
        } => send_message(&client, &id, &message, message_type.as_deref()).await?,
    };

    // Format and print output
    Ok((response, output_format))
}

/// Run message management commands.
pub async fn run_message_cmd(cmd: MessageCommand) -> Result<(CliResponse, OutputFormat)> {
    use crate::{message::*, ApiClient};

    // Get API base URL from environment or use default
    let api_base = std::env::var("NEOMIND_API_BASE")
        .unwrap_or_else(|_| "http://localhost:9375/api".to_string());

    // Create API client
    let client = ApiClient::with_base_url(&api_base);

    // Get output format (controlled by NEOMIND_JSON env var)
    let output_format = if std::env::var("NEOMIND_JSON").is_ok() {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    };

    let response = match cmd {
        MessageCommand::List {
            limit,
            offset,
            severity,
            status,
        } => {
            list_messages(
                &client,
                limit,
                offset,
                severity.as_deref(),
                status.as_deref(),
            )
            .await?
        }
        MessageCommand::Get { id } => get_message(&client, &id).await?,
        MessageCommand::Send {
            title,
            body,
            severity,
            source,
        } => send_message(&client, &title, &body, &severity, source.as_deref()).await?,
        MessageCommand::Read { id } => acknowledge_message(&client, &id).await?,
        MessageCommand::ChannelList => list_channels(&client).await?,
        MessageCommand::ChannelGet { name } => get_channel(&client, &name).await?,
        MessageCommand::ChannelTypes => list_channel_types(&client).await?,
        MessageCommand::ChannelTypeSchema { channel_type } => {
            get_channel_type_schema(&client, &channel_type).await?
        }
        MessageCommand::ChannelCreate {
            name,
            channel_type,
            config,
            enabled,
        } => create_channel(&client, &name, &channel_type, &config, enabled).await?,
        MessageCommand::ChannelUpdate { name, config } => {
            update_channel(&client, &name, &config).await?
        }
        MessageCommand::ChannelDelete { name } => delete_channel(&client, &name).await?,
        MessageCommand::ChannelTest { name } => test_channel(&client, &name).await?,
    };

    // Format and print output
    Ok((response, output_format))
}

/// Run push management commands.
pub async fn run_push_cmd(cmd: PushCommand) -> Result<(CliResponse, OutputFormat)> {
    use crate::{data_push::*, ApiClient};

    let api_base = std::env::var("NEOMIND_API_BASE")
        .unwrap_or_else(|_| "http://localhost:9375/api".to_string());
    let client = ApiClient::with_base_url(&api_base);
    let output_format = if std::env::var("NEOMIND_JSON").is_ok() {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    };

    let response = match cmd {
        PushCommand::List => list_targets(&client).await?,
        PushCommand::Get { id } => get_target(&client, &id).await?,
        PushCommand::Create {
            name,
            target_type,
            config,
            schedule,
            sources,
        } => {
            let t_type = target_type.as_deref().unwrap_or("webhook");
            let cfg = config.as_deref().unwrap_or("{}");
            let sched = schedule.as_deref().unwrap_or("event");
            let src = sources.as_deref().unwrap_or("");
            create_target(&client, &name, t_type, cfg, sched, src).await?
        }
        PushCommand::Update {
            id,
            name,
            config,
            enabled,
            sources,
            schedule,
            template,
            only_changes,
        } => {
            update_target(
                &client,
                &id,
                name.as_deref(),
                config.as_deref(),
                enabled,
                sources.as_deref(),
                schedule.as_deref(),
                template.as_deref(),
                only_changes,
            )
            .await?
        }
        PushCommand::Delete { id } => delete_target(&client, &id).await?,
        PushCommand::Start { id } => start_target(&client, &id).await?,
        PushCommand::Stop { id } => stop_target(&client, &id).await?,
        PushCommand::Enable { id } => start_target(&client, &id).await?,
        PushCommand::Disable { id } => stop_target(&client, &id).await?,
        PushCommand::Test { id } => test_target(&client, &id).await?,
        PushCommand::Logs { id, limit } => list_logs(&client, &id, Some(limit)).await?,
        PushCommand::Stats => get_stats(&client).await?,
    };

    Ok((response, output_format))
}

/// Run widget management commands.
pub async fn run_widget_cmd(cmd: WidgetCommand) -> Result<(CliResponse, OutputFormat)> {
    use crate::{widget::*, ApiClient};

    // Get API base URL from environment or use default
    let api_base = std::env::var("NEOMIND_API_BASE")
        .unwrap_or_else(|_| "http://localhost:9375/api".to_string());

    // Create API client
    let client = ApiClient::with_base_url(&api_base);

    // Get output format (controlled by NEOMIND_JSON env var)
    let output_format = if std::env::var("NEOMIND_JSON").is_ok() {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    };

    let (response, output_format) = match cmd {
        WidgetCommand::List {} => {
            let resp = list_widgets(&client).await?;
            (resp, output_format)
        }
        WidgetCommand::Get { id } => {
            let resp = get_widget(&client, &id).await?;
            (resp, output_format)
        }
        WidgetCommand::Bundle { id } => {
            let resp = get_widget_bundle(&client, &id).await?;
            (resp, output_format)
        }
        WidgetCommand::Create {
            name,
            widget_type,
            output,
            install,
        } => {
            let resp = create_widget(&name, &widget_type, output.as_deref())?;
            let resp = if install {
                // `widget create` wrote manifest.json + bundle.js under
                // `directory`; register them via the install path — no manual
                // packaging/tar needed (the create→install gap with a manual
                // tar step was agent friction even strong models stumbled on).
                let dir = resp
                    .data
                    .as_ref()
                    .and_then(|d| d.get("directory"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                if dir.is_empty() {
                    return Err(anyhow::anyhow!(
                        "widget create --install: could not resolve scaffold directory"
                    ));
                }
                install_widget_file(&client, &dir).await?
            } else {
                resp
            };
            (resp, output_format)
        }
        WidgetCommand::Install { file } => {
            (install_widget_file(&client, &file).await?, output_format)
        }
        WidgetCommand::Uninstall { id } => (uninstall_widget(&client, &id).await?, output_format),
        WidgetCommand::MarketList {} => {
            let resp = list_marketplace_widgets(&client).await?;
            (resp, output_format)
        }
        WidgetCommand::MarketInstall { id, version } => (
            install_widget_market(&client, &id, version.as_deref()).await?,
            output_format,
        ),
    };

    // Format and print output
    Ok((response, output_format))
}

pub async fn run_system_cmd(cmd: SystemCommand) -> Result<(CliResponse, OutputFormat)> {
    let client = crate::ApiClient::new();
    let base_format = if std::env::var("NEOMIND_JSON").is_ok() {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    };

    let result = match cmd {
        SystemCommand::Info {} => {
            let resp = crate::system::system_info(&client).await?;
            (resp, base_format)
        }
    };
    Ok(result)
}

pub async fn run_settings_cmd(cmd: SettingsCommand) -> Result<(CliResponse, OutputFormat)> {
    let client = crate::ApiClient::new();
    let base_format = if std::env::var("NEOMIND_JSON").is_ok() {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    };

    let result = match cmd {
        SettingsCommand::Timezone {} => {
            let resp = crate::settings::get_timezone(&client).await?;
            (resp, base_format)
        }
        SettingsCommand::SetTimezone { timezone } => {
            let resp = crate::settings::update_timezone(&client, &timezone).await?;
            (resp, base_format)
        }
        SettingsCommand::Timezones {} => {
            let resp = crate::settings::list_timezones(&client).await?;
            (resp, base_format)
        }
        SettingsCommand::Retention {} => {
            let resp = crate::settings::get_retention(&client).await?;
            (resp, base_format)
        }
        SettingsCommand::SetRetention {
            enabled,
            interval_hours,
            default_retention,
            image_retention,
        } => {
            let resp = crate::settings::update_retention(
                &client,
                enabled,
                interval_hours,
                default_retention,
                image_retention,
            )
            .await?;
            (resp, base_format)
        }
        SettingsCommand::Cleanup {} => {
            let resp = crate::settings::trigger_cleanup(&client).await?;
            (resp, base_format)
        }
    };
    Ok(result)
}

pub async fn run_connector_cmd(cmd: ConnectorCommand) -> Result<(CliResponse, OutputFormat)> {
    let client = crate::ApiClient::new();
    let base_format = if std::env::var("NEOMIND_JSON").is_ok() {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    };

    let result = match cmd {
        ConnectorCommand::List {} => {
            let resp = crate::connector::list_connectors(&client).await?;
            (resp, base_format)
        }
        ConnectorCommand::Get { id } => {
            let resp = crate::connector::get_connector(&client, &id).await?;
            (resp, base_format)
        }
        ConnectorCommand::Create {
            connector_type,
            name,
            host,
            port,
            tls,
            username,
            password,
            topics,
        } => {
            let resp = crate::connector::create_connector(
                &client,
                &name,
                Some(&connector_type),
                &host,
                port,
                tls, // Option<bool>; create_connector defaults to false when None
                username.as_deref(),
                password.as_deref(),
                topics.as_deref(),
            )
            .await?;
            (resp, base_format)
        }
        ConnectorCommand::Update {
            id,
            name,
            host,
            port,
            tls,
            username,
            password,
            topics,
            disable,
        } => {
            let enabled = disable.map(|d| !d);
            let tls_val = tls;
            let resp = crate::connector::update_connector(
                &client,
                &id,
                name.as_deref(),
                host.as_deref(),
                port,
                tls_val,
                username.as_deref(),
                password.as_deref(),
                topics.as_deref(),
                enabled,
            )
            .await?;
            (resp, base_format)
        }
        ConnectorCommand::Delete { id } => {
            let resp = crate::connector::delete_connector(&client, &id).await?;
            (resp, base_format)
        }
        ConnectorCommand::Enable { id } => {
            let resp = crate::connector::update_connector(
                &client,
                &id,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(true),
            )
            .await?;
            (resp, base_format)
        }
        ConnectorCommand::Disable { id } => {
            let resp = crate::connector::update_connector(
                &client,
                &id,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(false),
            )
            .await?;
            (resp, base_format)
        }
        ConnectorCommand::Test { id } => {
            let resp = crate::connector::test_connector(&client, &id).await?;
            (resp, base_format)
        }
        ConnectorCommand::Subscriptions {} => {
            let resp = crate::connector::list_subscriptions(&client).await?;
            (resp, base_format)
        }
        ConnectorCommand::Subscribe { topic, qos } => {
            let resp = crate::connector::subscribe_topic(&client, &topic, Some(qos)).await?;
            (resp, base_format)
        }
        ConnectorCommand::Unsubscribe { topic } => {
            let resp = crate::connector::unsubscribe_topic(&client, &topic).await?;
            (resp, base_format)
        }
    };
    Ok(result)
}

pub async fn run_login_cmd(
    data_dir: Option<String>,
    force: bool,
) -> Result<(CliResponse, OutputFormat)> {
    let fmt = if std::env::var("NEOMIND_JSON").is_ok() {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    };
    let resp = crate::auth_cmd::run_login(data_dir, force).await?;
    Ok((resp, fmt))
}

pub async fn run_logout_cmd() -> Result<(CliResponse, OutputFormat)> {
    let fmt = if std::env::var("NEOMIND_JSON").is_ok() {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    };
    let resp = crate::auth_cmd::run_logout().await?;
    Ok((resp, fmt))
}

pub async fn run_whoami_cmd() -> Result<(CliResponse, OutputFormat)> {
    let fmt = if std::env::var("NEOMIND_JSON").is_ok() {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    };
    let resp = crate::auth_cmd::run_whoami().await?;
    Ok((resp, fmt))
}

pub async fn run_user_cmd(user_cmd: UserCommand) -> Result<(CliResponse, OutputFormat)> {
    let fmt = if std::env::var("NEOMIND_JSON").is_ok() {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    };
    let resp = match user_cmd {
        UserCommand::ResetPassword { username, data_dir } => {
            crate::user_cmd::run_reset_password(data_dir, &username).await?
        }
    };
    Ok((resp, fmt))
}
