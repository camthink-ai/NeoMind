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
        Err(e) => return Err(DispatchError::Parse(e.to_string())),
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
