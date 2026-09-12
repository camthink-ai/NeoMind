//! In-process CLI dispatch.
//!
//! Allows the agent (and other in-process callers) to run `neomind` data
//! commands without spawning a subprocess. This eliminates the dependency on
//! whatever `neomind` binary happens to be in PATH, avoiding version drift
//! between the running server and the CLI binary the agent shells out to.
//!
//! Side-effecting / interactive top-level commands (`serve`, `chat`, `logs`,
//! ...) and local-only subcommands (e.g. `extension validate`, `api-key
//! create`) return [`DispatchError::NotInProcess`] so the caller can fall back
//! to spawning the real binary.

pub mod commands;
pub mod handlers;

use crate::types::CliResponse;
use clap::Parser;
use commands::{Args, Command};

/// Errors returned by [`dispatch`].
#[derive(Debug)]
pub enum DispatchError {
    /// The command cannot be executed in-process (it is side-effecting,
    /// interactive, or local-only). The caller should fall back to a subprocess.
    NotInProcess,
    /// Argument parsing failed. The string is clap's rendered error message.
    Parse(String),
    /// The underlying API request (or handler logic) failed.
    Api(String),
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DispatchError::NotInProcess => write!(f, "command cannot run in-process"),
            DispatchError::Parse(msg) => write!(f, "parse error: {}", msg),
            DispatchError::Api(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for DispatchError {}

/// Dispatch a tokenized `neomind` command in-process.
///
/// `argv` is the full argument vector including the program name as the first
/// element (e.g. `["neomind", "dashboard", "list"]`). Data commands
/// return their [`CliResponse`]; everything else returns
/// [`DispatchError::NotInProcess`] so the caller can fall back to a subprocess.
///
/// Uses `try_parse_from` so malformed input yields [`DispatchError::Parse`]
/// instead of `exit()`-ing the host process.
pub async fn dispatch(argv: &[String]) -> Result<CliResponse, DispatchError> {
    let parsed = match Args::try_parse_from(argv.iter()) {
        Ok(args) => args,
        Err(e) => {
            // First-shot flag errors from the agent's shell tool land here.
            // Counted from logs to rank which commands need surface fixes.
            tracing::warn!(
                target: "neomind::cli_dispatch",
                argv = ?argv,
                error = %e,
                "clap parse error"
            );
            return Err(DispatchError::Parse(e.to_string()));
        }
    };

    match parsed.command {
        // --- Side-effecting / interactive top-level commands ---
        Command::Serve { .. }
        | Command::Prompt { .. }
        | Command::Chat { .. }
        | Command::ListModels { .. }
        | Command::Health
        | Command::Logs { .. }
        | Command::CheckUpdate
        | Command::Upgrade { .. }
        | Command::Uninstall { .. } => Err(DispatchError::NotInProcess),

        // --- Local-only commands (need redb/auth from neomind-api, or print
        //     directly to stdout and rely on subprocess capture) ---
        Command::ApiKey { .. } => Err(DispatchError::NotInProcess),
        Command::User { .. } => Err(DispatchError::NotInProcess),
        Command::Extension { extension_cmd } => {
            if handlers::is_local_extension_command(&extension_cmd) {
                Err(DispatchError::NotInProcess)
            } else {
                let (resp, _fmt) = handlers::run_extension_cmd(extension_cmd)
                    .await
                    .map_err(|e| DispatchError::Api(e.to_string()))?;
                Ok(resp)
            }
        }

        // --- Pure data commands ---
        Command::Llm { llm_cmd } => {
            let (resp, _) = handlers::run_llm_cmd(llm_cmd)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Device { device_cmd } => {
            let (resp, _) = handlers::run_device_cmd(device_cmd)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Dashboard { dashboard_cmd } => {
            let (resp, _) = handlers::run_dashboard_cmd(dashboard_cmd)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Rule { rule_cmd } => {
            let (resp, _) = handlers::run_rule_cmd(rule_cmd)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Transform { transform_cmd } => {
            let (resp, _) = handlers::run_transform_cmd(transform_cmd)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Agent { agent_cmd } => {
            let (resp, _) = handlers::run_agent_cmd(agent_cmd)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Message { message_cmd } => {
            let (resp, _) = handlers::run_message_cmd(message_cmd)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Push { push_cmd } => {
            let (resp, _) = handlers::run_push_cmd(push_cmd)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Widget { widget_cmd } => {
            let (resp, _) = handlers::run_widget_cmd(widget_cmd)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::System { system_cmd } => {
            let (resp, _) = handlers::run_system_cmd(system_cmd)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Config { config_cmd } => {
            let (resp, _) = handlers::run_config_cmd(config_cmd)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Data { data_cmd } => {
            let (resp, _) = handlers::run_data_cmd(data_cmd)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Settings { settings_cmd } => {
            let (resp, _) = handlers::run_settings_cmd(settings_cmd)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Connector { connector_cmd } => {
            let (resp, _) = handlers::run_connector_cmd(connector_cmd)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Login { data_dir, force } => {
            let (resp, _) = handlers::run_login_cmd(data_dir, force)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Logout => {
            let (resp, _) = handlers::run_logout_cmd()
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Whoami => {
            let (resp, _) = handlers::run_whoami_cmd()
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        std::iter::once("neomind")
            .chain(parts.iter().copied())
            .map(str::to_string)
            .collect()
    }

    /// Side-effecting / interactive commands must return NotInProcess so the
    /// agent's shell tool falls back to a real subprocess — running `serve`
    /// in-process would wedge the host server forever.
    #[tokio::test]
    async fn side_effecting_commands_are_not_in_process() {
        for args in [
            vec!["serve"],
            vec!["serve", "--port", "0"],
            vec!["chat"],
            vec!["logs", "--follow"],
            vec!["upgrade"],
            vec!["health"],
        ] {
            let err = dispatch(&argv(&args)).await.unwrap_err();
            assert!(
                matches!(err, DispatchError::NotInProcess),
                "`neomind {:?}` must be NotInProcess, got {err:?}",
                args
            );
        }
    }

    /// Local-only commands (redb-backed or stdout-printing) also stay out of
    /// process.
    #[tokio::test]
    async fn local_only_commands_are_not_in_process() {
        for args in [vec!["api-key", "list"], vec!["user", "list"]] {
            let err = dispatch(&argv(&args)).await.unwrap_err();
            assert!(
                matches!(err, DispatchError::NotInProcess),
                "`neomind {:?}` must be NotInProcess, got {err:?}",
                args
            );
        }
    }

    /// Malformed input must yield Parse (so the caller can surface clap's
    /// message to the model for a corrected retry), never panic and never
    /// exit() the host process.
    #[tokio::test]
    async fn malformed_input_yields_parse_error() {
        for args in [
            vec!["device"],               // subcommand required
            vec!["device", "frobnicate"], // unknown subcommand
            vec!["--definitely-not-a-flag"],
            vec!["dashboard", "get"], // missing required ID positional
        ] {
            let err = dispatch(&argv(&args)).await.unwrap_err();
            assert!(
                matches!(err, DispatchError::Parse(_)),
                "`neomind {:?}` must be Parse, got {err:?}",
                args
            );
        }
    }

    /// A well-formed DATA command routes into the handler layer, which
    /// reports unreachability as a normal error CliResponse (not a dispatch
    /// error) — exactly what the agent's shell tool renders. With no server
    /// on the default base URL this is the "server down" path the incident
    /// agent would have hit; it must degrade to a message, never a panic.
    #[tokio::test]
    async fn data_command_degrades_to_error_response_without_server() {
        // Pin a port nothing listens on so the test never depends on (or
        // races with) a locally running dev server.
        std::env::set_var("NEOMIND_API_BASE", "http://127.0.0.1:9/test-api");
        let result = dispatch(&argv(&["device", "list"])).await;
        std::env::remove_var("NEOMIND_API_BASE");

        match result {
            Ok(resp) => {
                assert!(
                    !resp.success,
                    "no-server call must not report success: {resp:?}"
                );
            }
            Err(DispatchError::Api(msg)) => {
                assert!(!msg.is_empty(), "api error must carry a message");
            }
            Err(other) => panic!("expected Ok(error-response) or Api, got {other:?}"),
        }
    }
}
