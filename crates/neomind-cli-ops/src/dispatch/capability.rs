//! Proven read-only vs mutating classification for parsed `neomind` CLI.
//!
//! Only explicitly listed query variants are [`CliSideEffect::ProvenReadOnly`].
//! Everything else that parses is treated as mutating — new subcommands default
//! closed (must opt into the read-only allow-list). Unparsed / non-CLI shell
//! lines are classified by the agent as unknown side-effects, not here.

use super::commands::{
    AgentCommand, ApiKeyCommand, Args, Command, ConfigCommand, ConnectorCommand, DashboardCommand,
    DataCommand, DeviceCommand, DeviceTypeCommand, DraftCommand, ExtensionCommand, LlmCommand,
    MessageCommand, OperationCommand, PushCommand, RuleCommand, SettingsCommand, SystemCommand,
    TransformCommand, UserCommand, WidgetCommand,
};
use clap::Parser;
use serde_json::json;
use std::path::Path;

/// Side-effect class of a successfully parsed platform CLI command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliSideEffect {
    /// Proven query / inspection — safe to skip the mutating write gate.
    ProvenReadOnly,
    /// Creates, updates, deletes, invokes, or otherwise changes system state.
    Mutating,
}

/// Classify a fully parsed [`Command`] tree.
///
/// Default is [`CliSideEffect::Mutating`] — only allow-listed query variants
/// return [`CliSideEffect::ProvenReadOnly`].
pub fn classify_parsed_command(command: &Command) -> CliSideEffect {
    use CliSideEffect::*;
    match command {
        Command::Serve { .. }
        | Command::Prompt { .. }
        | Command::Chat { .. }
        | Command::Logs { .. }
        | Command::Upgrade { .. }
        | Command::Uninstall { .. }
        | Command::Login { .. }
        | Command::Logout
        | Command::CheckUpdate => Mutating,

        Command::ListModels { .. } | Command::Health | Command::Whoami => ProvenReadOnly,

        Command::User { user_cmd } => match user_cmd {
            UserCommand::List { .. } => ProvenReadOnly,
            _ => Mutating,
        },
        Command::ApiKey { key_cmd } => match key_cmd {
            ApiKeyCommand::List { .. } => ProvenReadOnly,
            _ => Mutating,
        },
        Command::Llm { llm_cmd } => match llm_cmd {
            LlmCommand::List { .. } | LlmCommand::Get { .. } | LlmCommand::Models { .. } => {
                ProvenReadOnly
            }
            _ => Mutating,
        },
        Command::Extension { extension_cmd } => match extension_cmd {
            ExtensionCommand::List { .. }
            | ExtensionCommand::Get { .. }
            | ExtensionCommand::Status { .. }
            | ExtensionCommand::Logs { .. }
            | ExtensionCommand::Validate { .. }
            | ExtensionCommand::Config { set: None, .. }
            | ExtensionCommand::MarketList { .. }
            | ExtensionCommand::Commands { .. } => ProvenReadOnly,
            // NM-240: classify by preparing a real PreparedInvoke and binding
            // Chat write-gate to that token. RO skip authorizes the ticket;
            // Mutating leaves it issued for authorize-on-Proceed. Cache-only
            // RO without a prepareable snapshot cannot skip the gate.
            ExtensionCommand::Invoke {
                extension_id,
                command,
                params,
                params_file,
                prepared_operation_id,
            } => {
                // Host ticket (NM-247) or legacy argv token: peek only (no remint).
                let host = super::host_context::peek_extension_invoke_host_context();
                let host_op = host.as_ref().and_then(|h| h.prepared_operation_id.clone());
                let op_peek = host_op.as_deref().or(prepared_operation_id.as_deref());
                if let Some(op_id) = op_peek {
                    return match neomind_core::extension::peek_prepared_invoke(op_id)
                        .map(|p| p.contract.effect)
                    {
                        Some(neomind_core::extension::CommandEffect::ReadOnly) => ProvenReadOnly,
                        Some(neomind_core::extension::CommandEffect::Mutating) | None => Mutating,
                    };
                }
                let host_params = host.and_then(|h| h.structured_params);
                let raw = if let Some(v) = host_params {
                    if v.is_null() {
                        serde_json::json!({})
                    } else if v.is_object() {
                        v
                    } else {
                        return Mutating;
                    }
                } else {
                    match load_classify_params(params.as_deref(), params_file.as_deref()) {
                        Ok(v) => v,
                        Err(_) => return Mutating,
                    }
                };
                // Effect only for bare CLI classify; Chat attach path injects the id.
                match neomind_core::extension::classify_and_bind_chat_invoke(
                    extension_id,
                    command,
                    &raw,
                )
                .effect
                {
                    neomind_core::extension::CommandEffect::ReadOnly => ProvenReadOnly,
                    neomind_core::extension::CommandEffect::Mutating => Mutating,
                }
            }
            _ => Mutating,
        },
        Command::Device { device_cmd } => classify_device(device_cmd),
        Command::Dashboard { dashboard_cmd } => match dashboard_cmd {
            DashboardCommand::List { .. } | DashboardCommand::Get { .. } => ProvenReadOnly,
            _ => Mutating,
        },
        Command::Rule { rule_cmd } => match rule_cmd {
            RuleCommand::List
            | RuleCommand::Get { .. }
            | RuleCommand::History { .. }
            | RuleCommand::Explain { .. } => ProvenReadOnly,
            _ => Mutating,
        },
        Command::Transform { transform_cmd } => match transform_cmd {
            TransformCommand::List
            | TransformCommand::Get { .. }
            | TransformCommand::Executions { .. }
            | TransformCommand::Metrics
            | TransformCommand::DataSources => ProvenReadOnly,
            _ => Mutating,
        },
        Command::Agent { agent_cmd } => match agent_cmd {
            AgentCommand::List
            | AgentCommand::Get { .. }
            | AgentCommand::Executions { .. }
            | AgentCommand::LatestExecution { .. }
            | AgentCommand::Conversation { .. }
            | AgentCommand::Memory { .. }
            | AgentCommand::PlanPreview { .. }
            | AgentCommand::Inventory { .. } => ProvenReadOnly,
            _ => Mutating,
        },
        Command::Message { message_cmd } => match message_cmd {
            MessageCommand::List { .. }
            | MessageCommand::Get { .. }
            | MessageCommand::ChannelList
            | MessageCommand::ChannelGet { .. }
            | MessageCommand::ChannelTypes
            | MessageCommand::ChannelTypeSchema { .. } => ProvenReadOnly,
            _ => Mutating,
        },
        Command::Push { push_cmd } => match push_cmd {
            PushCommand::List
            | PushCommand::Get { .. }
            | PushCommand::Logs { .. }
            | PushCommand::Stats => ProvenReadOnly,
            _ => Mutating,
        },
        Command::Widget { widget_cmd } => match widget_cmd {
            WidgetCommand::List { .. }
            | WidgetCommand::Get { .. }
            | WidgetCommand::MarketList { .. } => ProvenReadOnly,
            _ => Mutating,
        },
        Command::System { system_cmd } => match system_cmd {
            SystemCommand::Info { .. }
            | SystemCommand::Health { .. }
            | SystemCommand::Stats { .. } => ProvenReadOnly,
        },
        Command::Data { data_cmd } => match data_cmd {
            DataCommand::List { .. } => ProvenReadOnly,
        },
        Command::Config { config_cmd } => match config_cmd {
            ConfigCommand::Export { .. } | ConfigCommand::Validate { .. } => ProvenReadOnly,
            _ => Mutating,
        },
        Command::Settings { settings_cmd } => match settings_cmd {
            SettingsCommand::Timezone {}
            | SettingsCommand::Timezones {}
            | SettingsCommand::Retention {} => ProvenReadOnly,
            _ => Mutating,
        },
        Command::Connector { connector_cmd } => match connector_cmd {
            ConnectorCommand::List { .. }
            | ConnectorCommand::Get { .. }
            | ConnectorCommand::Subscriptions { .. } => ProvenReadOnly,
            _ => Mutating,
        },
        Command::Operation { operation_cmd } => match operation_cmd {
            OperationCommand::Get { .. } | OperationCommand::List { .. } => ProvenReadOnly,
        },
    }
}

fn classify_device(device_cmd: &DeviceCommand) -> CliSideEffect {
    use CliSideEffect::*;
    match device_cmd {
        DeviceCommand::List { .. }
        | DeviceCommand::Get { .. }
        | DeviceCommand::Latest { .. }
        | DeviceCommand::History { .. }
        | DeviceCommand::WebhookUrl { .. } => ProvenReadOnly,
        DeviceCommand::Types {
            type_cmd: DeviceTypeCommand::List | DeviceTypeCommand::Get { .. },
        }
        | DeviceCommand::Drafts {
            draft_cmd: DraftCommand::List { .. } | DraftCommand::Get { .. },
        } => ProvenReadOnly,
        _ => Mutating,
    }
}

/// Best-effort params load for Chat classify (no HTTP). Invalid JSON → Err
/// so invoke fails closed to Mutating.
fn load_classify_params(
    params_json: Option<&str>,
    params_file: Option<&Path>,
) -> Result<serde_json::Value, String> {
    if let Some(path) = params_file {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let v = crate::json_arg::parse_json_arg_str(text.trim())?;
        if !v.is_object() && !v.is_null() {
            return Err("params file must be a JSON object".into());
        }
        return Ok(if v.is_null() { json!({}) } else { v });
    }
    match params_json {
        None => Ok(json!({})),
        Some(s) => {
            let v = crate::json_arg::parse_json_arg_str(s)?;
            if !v.is_object() && !v.is_null() {
                return Err("params must be a JSON object".into());
            }
            Ok(if v.is_null() { json!({}) } else { v })
        }
    }
}

/// Parse `argv` (including leading `neomind`) and classify side effects.
///
/// `Err` means clap could not parse — caller must treat as unknown side-effects
/// (not proven read-only).
///
/// `--help` / `-V` are clap DisplayHelp/DisplayVersion "errors". They are
/// **documentation only** — always [`CliSideEffect::ProvenReadOnly`], never
/// unknown→mutating. Treating help as parse failure previously sent
/// `extension commands <id> --help` through the write gate / PreferKnown.
pub fn classify_argv(argv: &[String]) -> Result<CliSideEffect, String> {
    match Args::try_parse_from(argv.iter()) {
        Ok(args) => Ok(classify_parsed_command(&args.command)),
        Err(e) => {
            use clap::error::ErrorKind;
            if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) {
                return Ok(CliSideEffect::ProvenReadOnly);
            }
            Err(e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify_line(line: &str) -> CliSideEffect {
        let argv: Vec<String> = std::iter::once("neomind".to_string())
            .chain(line.split_whitespace().map(str::to_string))
            .collect();
        classify_argv(&argv).expect(line)
    }

    #[test]
    fn proven_queries_are_read_only() {
        assert_eq!(classify_line("device list"), CliSideEffect::ProvenReadOnly);
        assert_eq!(
            classify_line("device get cam-1"),
            CliSideEffect::ProvenReadOnly
        );
        assert_eq!(
            classify_line("rule explain r1"),
            CliSideEffect::ProvenReadOnly
        );
        assert_eq!(
            classify_line("dashboard list"),
            CliSideEffect::ProvenReadOnly
        );
        assert_eq!(classify_line("health"), CliSideEffect::ProvenReadOnly);
        assert_eq!(classify_line("llm models"), CliSideEffect::ProvenReadOnly);
    }

    /// `agent invoke` through shell must enter the mutating write gate —
    /// the durable anti-duplicate path for agent runs (pinned together with
    /// the write-fingerprint test in neomind-agent task_continuity).
    #[test]
    fn agent_invoke_is_mutating() {
        assert_eq!(
            classify_line("agent invoke agent-001"),
            CliSideEffect::Mutating
        );
        assert_eq!(
            classify_line("agent invoke agent-001 recheck"),
            CliSideEffect::Mutating
        );
    }

    #[test]
    fn mutations_are_not_read_only() {
        assert_eq!(
            classify_line("device delete cam-1"),
            CliSideEffect::Mutating
        );
        assert_eq!(
            classify_line("device control cam-1 reboot"),
            CliSideEffect::Mutating
        );
        assert_eq!(
            classify_line("extension reload weather-forecast-v2"),
            CliSideEffect::Mutating
        );
        assert_eq!(
            classify_line("device write-metric cam-1 --metric temperature --value 1"),
            CliSideEffect::Mutating
        );
        assert_eq!(
            classify_line("transform test-code --code return1 --input {}"),
            CliSideEffect::Mutating
        );
    }

    #[test]
    fn unknown_argv_is_err_not_read_only() {
        let argv = vec![
            "neomind".into(),
            "totally-bogus-domain".into(),
            "list".into(),
        ];
        assert!(classify_argv(&argv).is_err());
    }

    #[test]
    fn extension_commands_is_read_only() {
        assert_eq!(
            classify_line("extension commands weather-forecast-v2"),
            CliSideEffect::ProvenReadOnly
        );
    }

    #[test]
    fn extension_logs_validate_readonly_config_and_push_logs_are_read_only() {
        assert_eq!(
            classify_line("extension logs weather-forecast-v2 --lines 50"),
            CliSideEffect::ProvenReadOnly
        );
        assert_eq!(
            classify_line("extension config weather-forecast-v2"),
            CliSideEffect::ProvenReadOnly
        );
        assert_eq!(
            classify_line("extension validate ./pkg.nep"),
            CliSideEffect::ProvenReadOnly
        );
        assert_eq!(
            classify_line("push logs target-1 --limit 20"),
            CliSideEffect::ProvenReadOnly
        );
        // Writing config with --set must stay Mutating.
        assert_eq!(
            classify_line("extension config weather-forecast-v2 --set {\"city\":\"Beijing\"}"),
            CliSideEffect::Mutating
        );
    }

    /// P1: `--help` on a proven query must not fail classify (unknown→mutating).
    #[test]
    fn extension_commands_help_is_read_only() {
        let argv = vec![
            "neomind".into(),
            "extension".into(),
            "commands".into(),
            "weather-forecast-v2".into(),
            "--help".into(),
        ];
        assert_eq!(
            classify_argv(&argv).expect("help must classify"),
            CliSideEffect::ProvenReadOnly
        );
    }

    /// Help on a mutating verb is still documentation — not a write.
    #[test]
    fn mutating_verb_help_is_documentation_read_only() {
        let argv = vec![
            "neomind".into(),
            "extension".into(),
            "reload".into(),
            "weather-forecast-v2".into(),
            "--help".into(),
        ];
        assert_eq!(
            classify_argv(&argv).expect("help must classify"),
            CliSideEffect::ProvenReadOnly,
            "documentation must not enter the write gate"
        );
    }

    #[test]
    fn extension_invoke_classify_is_conservative_without_cache() {
        // Unique instance so other tests' remember_capability cannot leak.
        // Never registered → no published snapshot → Mutating (name-only
        // grants forbidden). Distinct from CLI-only cold start after register.
        assert_eq!(
            classify_line(
                "extension invoke never-cached-ext-xyz get_weather --params {\"city\":\"Xiamen\"}"
            ),
            CliSideEffect::Mutating
        );
        assert_eq!(
            classify_line(
                "extension invoke never-cached-ext-xyz set_default_city --params {\"city\":\"Xiamen\"}"
            ),
            CliSideEffect::Mutating
        );
    }

    #[test]
    fn extension_invoke_respects_remembered_capability() {
        // NM-240: RO skip requires a prepareable snapshot (not cache-only
        // remember_capability). Publish a load snapshot so classify can
        // prepare+authorize a real PreparedInvoke.
        use neomind_core::extension::{
            begin_instance_generation, invalidate_instance_capability, CommandEffect,
            ExtensionCommand, ExtensionCommandSnapshot, MetricDataType, ParameterDefinition,
        };
        let id = "cached-weather-ext";
        invalidate_instance_capability(id);
        let gen = begin_instance_generation(id);
        let get = ExtensionCommand {
            name: "get_weather".into(),
            display_name: "Get Weather".into(),
            description: "Query weather".into(),
            parameters: vec![ParameterDefinition {
                name: "city".into(),
                display_name: "City".into(),
                description: "City name".into(),
                param_type: MetricDataType::String,
                required: true,
                default_value: None,
                min: None,
                max: None,
                options: vec![],
            }],
            effect: Some(CommandEffect::ReadOnly),
            replay_safety: None,
            execution: None,
            ..Default::default()
        };
        ExtensionCommandSnapshot::build(id, "1.0.0", None, vec![get], gen)
            .publish_capabilities()
            .unwrap();
        assert_eq!(
            classify_line(&format!(
                "extension invoke {id} get_weather --params {{\"city\":\"Xiamen\"}}"
            )),
            CliSideEffect::ProvenReadOnly
        );
        invalidate_instance_capability(id);
    }

    /// NM-235: CLI classify after load-snapshot publish (no prepare / tool).
    #[test]
    fn extension_invoke_respects_published_snapshot_without_prepare() {
        use neomind_core::extension::{
            begin_instance_generation, invalidate_instance_capability, CommandEffect,
            ExtensionCommand, ExtensionCommandSnapshot, MetricDataType, ParameterDefinition,
        };
        let id = "snapshot-weather-ext";
        invalidate_instance_capability(id);
        let gen = begin_instance_generation(id);
        let get = ExtensionCommand {
            name: "get_weather".into(),
            display_name: "Get Weather".into(),
            description: "Query weather".into(),
            parameters: vec![ParameterDefinition {
                name: "city".into(),
                display_name: "City".into(),
                description: "City name".into(),
                param_type: MetricDataType::String,
                required: true,
                default_value: None,
                min: None,
                max: None,
                options: vec![],
            }],
            effect: Some(CommandEffect::ReadOnly),
            replay_safety: None,
            execution: None,
            ..Default::default()
        };
        let set = ExtensionCommand {
            name: "set_default_city".into(),
            effect: None,
            replay_safety: None,
            execution: None,
            ..Default::default()
        };
        ExtensionCommandSnapshot::build(id, "1.0.0", None, vec![get, set], gen)
            .publish_capabilities()
            .unwrap();

        assert_eq!(
            classify_line(&format!(
                "extension invoke {id} get_weather --params {{\"city\":\"Xiamen\"}}"
            )),
            CliSideEffect::ProvenReadOnly
        );
        assert_eq!(
            classify_line(&format!(
                "extension invoke {id} set_default_city --params {{\"city\":\"Xiamen\"}}"
            )),
            CliSideEffect::Mutating
        );
        invalidate_instance_capability(id);
    }

    /// P2-3 anti-drift check: capability classification must remain fail-closed
    /// for unknown/unparsed/mutating commands while covering every known
    /// read-only query subcommand across all CLI domains.
    #[test]
    fn capability_is_fail_closed_and_covers_all_known_read_only_queries() {
        let read_only_commands = [
            "health",
            "whoami",
            "list-models",
            "device list",
            "device get dev-1",
            "device latest dev-1",
            "device history dev-1",
            "device webhook-url dev-1",
            "device types list",
            "device types get temp_sensor",
            "device drafts list",
            "device drafts get draft-1",
            "dashboard list",
            "dashboard get dash-1",
            "rule list",
            "rule get rule-1",
            "rule history rule-1",
            "rule explain rule-1",
            "agent list",
            "agent get agent-1",
            "agent executions agent-1",
            "agent latest-execution agent-1",
            "agent conversation agent-1",
            "agent memory agent-1",
            "extension list",
            "extension get weather",
            "extension get weather --offline",
            "extension status weather",
            "extension logs weather",
            "extension validate ./ext.nep",
            "extension config weather",
            "extension market-list",
            "extension commands weather",
            "transform list",
            "transform get tr-1",
            "transform executions tr-1",
            "transform metrics",
            "transform data-sources",
            "widget list",
            "widget get value-card",
            "widget market-list",
            "message list",
            "message get msg-1",
            "message channel-list",
            "message channel-get ops",
            "message channel-types",
            "message channel-type-schema webhook",
            "push list",
            "push get target-1",
            "push logs target-1",
            "push stats",
            "llm list",
            "llm get backend-1",
            "llm models",
            "connector list",
            "connector get conn-1",
            "connector subscriptions",
            "settings timezone",
            "settings timezones",
            "settings retention",
            "operation get op-1",
            "operation list",
            "system info",
            "api-key list",
            "user list",
            "data list",
            "config export",
            "config validate ./config.toml",
        ];
        for cmd in read_only_commands {
            assert_eq!(
                classify_line(cmd),
                CliSideEffect::ProvenReadOnly,
                "known read-only command `{cmd}` must classify as ProvenReadOnly"
            );
        }

        let mutating_commands = [
            "device create --name s1 --device-type sensor",
            "device update dev-1 --name s2",
            "device delete dev-1",
            "device control dev-1 reboot",
            "device write-metric dev-1 --metric temp --value 25",
            "dashboard create --name d1",
            "dashboard delete dash-1",
            "rule delete rule-1",
            "rule enable rule-1",
            "rule disable rule-1",
            "agent invoke agent-1",
            "agent delete agent-1",
            "extension reload weather",
            "extension config weather --set {}",
            "extension install ./ext.nep",
            "extension install ./ext.nep --offline",
            "extension uninstall weather",
            "extension uninstall weather --offline",
            "transform delete tr-1",
            "widget uninstall my-widget",
            "message send --title hi --body body",
            "message delete msg-1",
            "push delete target-1",
            "llm activate backend-1",
            "connector delete conn-1",
            "settings set-timezone UTC",
            "settings cleanup",
            "api-key delete key-1",
        ];
        for cmd in mutating_commands {
            assert_eq!(
                classify_line(cmd),
                CliSideEffect::Mutating,
                "mutating command `{cmd}` must classify as Mutating"
            );
        }

        // Fail-closed: unknown domain or unknown action must return Err.
        for bogus in [
            vec!["neomind", "unknown-domain", "list"],
            vec!["neomind", "device", "unknown-action"],
            vec!["neomind", "extension", "info-nonexistent"],
        ] {
            let argv: Vec<String> = bogus.into_iter().map(str::to_string).collect();
            assert!(
                classify_argv(&argv).is_err(),
                "unparsed argv {argv:?} must fail closed with Err"
            );
        }
    }
}
