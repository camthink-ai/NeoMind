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
            | ExtensionCommand::MarketList { .. } => ProvenReadOnly,
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
pub fn classify_argv(argv: &[String]) -> Result<CliSideEffect, String> {
    let args = Args::try_parse_from(argv.iter()).map_err(|e| e.to_string())?;
    Ok(classify_parsed_command(&args.command))
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
}
