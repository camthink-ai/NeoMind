//! Proven read-only vs mutating classification for parsed `neomind` CLI.
//!
//! Only explicitly listed query variants are [`CliSideEffect::ProvenReadOnly`].
//! Everything else that parses is treated as mutating — new subcommands default
//! closed (must opt into the read-only allow-list). Unparsed / non-CLI shell
//! lines are classified by the agent as unknown side-effects, not here.

use super::commands::{
    AgentCommand, ApiKeyCommand, Args, Command, ConfigCommand, ConnectorCommand, DashboardCommand,
    DataCommand, DeviceCommand, DeviceTypeCommand, DraftCommand, ExtensionCommand, LlmCommand,
    MessageCommand, PushCommand, RuleCommand, SettingsCommand, SystemCommand, TransformCommand,
    UserCommand, WidgetCommand,
};
use clap::Parser;

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
            | ExtensionCommand::MarketList { .. }
            | ExtensionCommand::Commands { .. } => ProvenReadOnly,
            // Invoke capability comes from the unified command contract
            // (host override / declared effect) — not from command-name
            // heuristics. Unknown commands stay Mutating.
            ExtensionCommand::Invoke {
                extension_id,
                command,
                ..
            } => match neomind_core::extension::sync_effect_for_invoke(extension_id, command) {
                neomind_core::extension::CommandEffect::ReadOnly => ProvenReadOnly,
                neomind_core::extension::CommandEffect::Mutating => Mutating,
            },
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
            | AgentCommand::Memory { .. } => ProvenReadOnly,
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
            PushCommand::List | PushCommand::Get { .. } | PushCommand::Stats => ProvenReadOnly,
            _ => Mutating,
        },
        Command::Widget { widget_cmd } => match widget_cmd {
            WidgetCommand::List { .. }
            | WidgetCommand::Get { .. }
            | WidgetCommand::MarketList { .. } => ProvenReadOnly,
            _ => Mutating,
        },
        Command::System { system_cmd } => match system_cmd {
            SystemCommand::Info { .. } => ProvenReadOnly,
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
        use neomind_core::extension::{
            begin_instance_generation, remember_capability, CommandEffect, CommandExecutionKind,
            CommandReplaySafety,
        };
        let gen = begin_instance_generation("cached-weather-ext");
        remember_capability(
            "cached-weather-ext",
            "get_weather",
            CommandEffect::ReadOnly,
            CommandReplaySafety::Safe,
            CommandExecutionKind::Immediate,
            &gen,
        )
        .unwrap();
        assert_eq!(
            classify_line(
                "extension invoke cached-weather-ext get_weather --params {\"city\":\"Xiamen\"}"
            ),
            CliSideEffect::ProvenReadOnly
        );
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
}
