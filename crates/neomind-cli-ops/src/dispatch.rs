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

pub mod capability;
pub mod commands;
pub mod handlers;
pub mod host_context;

pub use capability::{classify_argv, classify_parsed_command, CliSideEffect};
pub use host_context::{
    peek_extension_invoke_host_context, set_extension_invoke_host_context,
    take_extension_invoke_host_context, with_extension_invoke_host_context,
    ExtensionInvokeHostContext,
};

use crate::types::{CliResponse, OutputFormat};
use clap::Parser;
use commands::{Args, Command};

/// Explicit execution context for in-process and CLI dispatch.
///
/// Carries API base URL, API key, and output format explicitly so callers
/// never need to mutate process-global environment variables (`std::env::set_var`)
/// on async hot paths.
#[derive(Debug, Clone, Default)]
pub struct DispatchContext {
    /// Explicit API base URL (e.g. `http://localhost:9375/api`).
    /// When `None`, falls back to `NEOMIND_API_BASE` or the default base URL.
    pub api_base: Option<String>,
    /// Explicit API key (Bearer token).
    /// When `None`, falls back to `NEOMIND_API_KEY` or auto-auth.
    pub api_key: Option<String>,
    /// Explicit output format.
    /// When `None`, falls back to `NEOMIND_JSON` (`Json` if set, else `Human`).
    pub output_format: Option<OutputFormat>,
}

impl DispatchContext {
    /// Build a context from the current process environment without mutating it.
    pub fn from_env() -> Self {
        Self {
            api_base: std::env::var("NEOMIND_API_BASE").ok(),
            api_key: std::env::var("NEOMIND_API_KEY").ok(),
            output_format: if std::env::var("NEOMIND_JSON").is_ok() {
                Some(OutputFormat::Json)
            } else {
                None
            },
        }
    }

    /// Build an [`crate::ApiClient`] honoring this context's base URL and API key.
    pub fn api_client(&self) -> crate::ApiClient {
        let base = self
            .api_base
            .clone()
            .or_else(|| std::env::var("NEOMIND_API_BASE").ok())
            .unwrap_or_else(|| "http://localhost:9375/api".to_string());
        crate::ApiClient::with_base_url_and_key(&base, self.api_key.clone())
    }

    /// Resolve the effective output format.
    pub fn output_format(&self) -> OutputFormat {
        self.output_format.unwrap_or_else(|| {
            if std::env::var("NEOMIND_JSON").is_ok() {
                OutputFormat::Json
            } else {
                OutputFormat::Human
            }
        })
    }
}

/// Errors returned by [`dispatch`] and [`dispatch_with_context`].
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

/// Dispatch a tokenized `neomind` command in-process using default environment
/// resolution.
///
/// Prefer [`dispatch_with_context`] from concurrent async callers so API base,
/// API key, and output format are passed explicitly without mutating process
/// environment variables.
pub async fn dispatch(argv: &[String]) -> Result<CliResponse, DispatchError> {
    dispatch_with_context(argv, &DispatchContext::default()).await
}

/// Dispatch a tokenized `neomind` command in-process with an explicit
/// [`DispatchContext`].
///
/// `argv` is the full argument vector including the program name as the first
/// element (e.g. `["neomind", "dashboard", "list"]`). Data commands
/// return their [`CliResponse`]; everything else returns
/// [`DispatchError::NotInProcess`] so the caller can fall back to a subprocess.
///
/// Uses `try_parse_from` so malformed input yields [`DispatchError::Parse`]
/// instead of `exit()`-ing the host process.
pub async fn dispatch_with_context(
    argv: &[String],
    ctx: &DispatchContext,
) -> Result<CliResponse, DispatchError> {
    let parsed = match Args::try_parse_from(argv.iter()) {
        Ok(args) => args,
        Err(e) => {
            // `--help` / `--version` arrive here as clap ERRORS
            // (DisplayHelp/DisplayVersion) — but the REAL CLI exits 0 with
            // the rendered text. Classifying them as parse errors made every
            // `... --help` read as a failure to the agent (exit 2, "error:"
            // prefix) and triggered identical-command retries. Return the
            // rendered text as a SUCCESS: same observable contract as the
            // binary.
            use clap::error::ErrorKind;
            if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) {
                // P0-A: `extension commands <id> --help` is a common small-model
                // discovery form. Clap meta-help for the verb is false progress
                // (exit 0, no command list). Redirect to the real commands
                // payload when an id is present. Bare `extension commands --help`
                // stays Usage text.
                if e.kind() == ErrorKind::DisplayHelp {
                    if let Some(id) = extension_commands_id_from_help_argv(argv) {
                        return redirect_extension_commands_help(&id, ctx).await;
                    }
                }
                return Ok(CliResponse::success(
                    serde_json::json!({ "help": e.to_string() }),
                    "help requested (exit 0 — this is rendered output, not an error)",
                ));
            }
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

    // Respect explicit `--json` / `--format json|text` when the caller did
    // not pin a format. `--json` is a SetTrue alias for format=json (Day9
    // haccp timeout: models burn rounds on rejected bare `--json`).
    let effective_ctx = if ctx.output_format.is_none() {
        if parsed.json || parsed.format.as_deref() == Some("json") {
            let mut c = ctx.clone();
            c.output_format = Some(OutputFormat::Json);
            c
        } else if parsed.format.as_deref() == Some("text") {
            let mut c = ctx.clone();
            c.output_format = Some(OutputFormat::Human);
            c
        } else {
            ctx.clone()
        }
    } else {
        ctx.clone()
    };

    match parsed.command {
        // --- Side-effecting / interactive top-level commands ---
        Command::Serve { .. }
        | Command::Prompt { .. }
        | Command::Chat { .. }
        | Command::ListModels { .. }
        | Command::Logs { .. }
        | Command::CheckUpdate
        | Command::Upgrade { .. }
        | Command::Uninstall { .. } => Err(DispatchError::NotInProcess),

        // --- api-key / user: IN-PROCESS capable. api-key goes through the
        //     running server's HTTP endpoints (see run_api_key_cmd — the key
        //     store must have exactly one writer); user list/set-role operate
        //     directly on the auth redb (same data dir). reset-password is
        //     the one exception: it reads a hidden password from stdin,
        //     which an in-process caller (agent shell / tauri pre-dispatch)
        //     cannot supply — keep it out-of-process. ---
        Command::ApiKey { key_cmd } => {
            let (resp, _) = handlers::run_api_key_cmd_with_context(key_cmd, &effective_ctx)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::User { user_cmd } => {
            if matches!(user_cmd, commands::UserCommand::ResetPassword { .. }) {
                Err(DispatchError::NotInProcess)
            } else {
                let (resp, _) = handlers::run_user_cmd_with_context(user_cmd, &effective_ctx)
                    .await
                    .map_err(|e| DispatchError::Api(e.to_string()))?;
                Ok(resp)
            }
        }
        Command::Extension { extension_cmd } => {
            if handlers::is_local_extension_command(&extension_cmd) {
                Err(DispatchError::NotInProcess)
            } else {
                let (resp, _fmt) =
                    handlers::run_extension_cmd_with_context(extension_cmd, &effective_ctx)
                        .await
                        .map_err(|e| DispatchError::Api(e.to_string()))?;
                Ok(resp)
            }
        }

        // --- Pure data commands ---
        Command::Llm { llm_cmd } => {
            let (resp, _) = handlers::run_llm_cmd_with_context(llm_cmd, &effective_ctx)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Device { device_cmd } => {
            let (resp, _) = handlers::run_device_cmd_with_context(device_cmd, &effective_ctx)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Dashboard { dashboard_cmd } => {
            let (resp, _) = handlers::run_dashboard_cmd_with_context(dashboard_cmd, &effective_ctx)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Rule { rule_cmd } => {
            let (resp, _) = handlers::run_rule_cmd_with_context(rule_cmd, &effective_ctx)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Transform { transform_cmd } => {
            let (resp, _) = handlers::run_transform_cmd_with_context(transform_cmd, &effective_ctx)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Agent { agent_cmd } => {
            let (resp, _) = handlers::run_agent_cmd_with_context(agent_cmd, &effective_ctx)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Message { message_cmd } => {
            let (resp, _) = handlers::run_message_cmd_with_context(message_cmd, &effective_ctx)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Push { push_cmd } => {
            let (resp, _) = handlers::run_push_cmd_with_context(push_cmd, &effective_ctx)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Widget { widget_cmd } => {
            let (resp, _) = handlers::run_widget_cmd_with_context(widget_cmd, &effective_ctx)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Health => {
            let client = effective_ctx.api_client();
            crate::system::health_check(&client)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))
        }
        Command::System { system_cmd } => {
            let (resp, _) = handlers::run_system_cmd_with_context(system_cmd, &effective_ctx)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Config { config_cmd } => {
            let (resp, _) = handlers::run_config_cmd_with_context(config_cmd, &effective_ctx)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Data { data_cmd } => {
            let (resp, _) = handlers::run_data_cmd_with_context(data_cmd, &effective_ctx)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Settings { settings_cmd } => {
            let (resp, _) = handlers::run_settings_cmd_with_context(settings_cmd, &effective_ctx)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Operation { operation_cmd } => {
            let (resp, _) = handlers::run_operation_cmd_with_context(operation_cmd, &effective_ctx)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Connector { connector_cmd } => {
            let (resp, _) = handlers::run_connector_cmd_with_context(connector_cmd, &effective_ctx)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Login { data_dir, force } => {
            let (resp, _) = handlers::run_login_cmd_with_context(data_dir, force, &effective_ctx)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Logout => {
            let (resp, _) = handlers::run_logout_cmd_with_context(&effective_ctx)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
        Command::Whoami => {
            let (resp, _) = handlers::run_whoami_cmd_with_context(&effective_ctx)
                .await
                .map_err(|e| DispatchError::Api(e.to_string()))?;
            Ok(resp)
        }
    }
}

/// Extract `<id>` from `neomind extension commands <id> … --help|-h …`.
///
/// Returns `None` for bare `extension commands --help` (no id) so callers keep
/// clap Usage. Ignores known global flags (`-v`, `--verbose`, `--format`).
fn extension_commands_id_from_help_argv(argv: &[String]) -> Option<String> {
    let mut iter = argv.iter().map(String::as_str);
    // Program name.
    iter.next()?;
    if iter.next()? != "extension" {
        return None;
    }
    if iter.next()? != "commands" {
        return None;
    }
    let mut id: Option<String> = None;
    let mut saw_help = false;
    while let Some(tok) = iter.next() {
        match tok {
            "--help" | "-h" => saw_help = true,
            "-v" | "--verbose" => {}
            "--search" => {
                // Consume search query; do not treat it as id.
                let _ = iter.next();
            }
            flag if flag.starts_with("--search=") => {}
            "--format" => {
                // Consume value when present.
                let _ = iter.next();
            }
            flag if flag.starts_with("--format=") => {}
            flag if flag.starts_with('-') => {
                // Unknown flag — do not treat a following token as id.
            }
            positional if id.is_none() => id = Some(positional.to_string()),
            _ => {}
        }
    }
    if saw_help {
        id
    } else {
        None
    }
}

const EXTENSION_COMMANDS_HELP_REDIRECT_NOTE: &str = "\
`--help` on `extension commands <id>` is CLI-verb Usage, not the command list. \
Returning the machine-readable command list instead. Discovery: \
`neomind extension commands <id>` (no --help); invoke via shell \
`{command:\"neomind extension invoke <id> <command>\", params:{...}}`.";

/// Run `extension commands <id>` and annotate that `--help` was redirected.
async fn redirect_extension_commands_help(
    id: &str,
    ctx: &DispatchContext,
) -> Result<CliResponse, DispatchError> {
    use commands::ExtensionCommand;
    let (mut resp, _) = handlers::run_extension_cmd_with_context(
        ExtensionCommand::Commands {
            id: Some(id.to_string()),
            search: None,
        },
        ctx,
    )
    .await
    .map_err(|e| DispatchError::Api(e.to_string()))?;

    match &mut resp.data {
        Some(serde_json::Value::Object(map)) => {
            map.insert("_help_redirect".into(), serde_json::json!(true));
            map.insert(
                "note".into(),
                serde_json::json!(EXTENSION_COMMANDS_HELP_REDIRECT_NOTE),
            );
        }
        Some(other) => {
            resp.data = Some(serde_json::json!({
                "_help_redirect": true,
                "note": EXTENSION_COMMANDS_HELP_REDIRECT_NOTE,
                "data": other,
            }));
        }
        None => {
            resp.data = Some(serde_json::json!({
                "_help_redirect": true,
                "note": EXTENSION_COMMANDS_HELP_REDIRECT_NOTE,
            }));
        }
    }
    let suggestion = match resp.suggestion.take() {
        Some(s) => format!("{EXTENSION_COMMANDS_HELP_REDIRECT_NOTE} {s}"),
        None => EXTENSION_COMMANDS_HELP_REDIRECT_NOTE.to_string(),
    };
    Ok(resp.with_suggestion(suggestion))
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

    #[test]
    fn extension_commands_help_extracts_id() {
        assert_eq!(
            extension_commands_id_from_help_argv(&argv(&[
                "extension",
                "commands",
                "weather-forecast-v2",
                "--help"
            ])),
            Some("weather-forecast-v2".into())
        );
        assert_eq!(
            extension_commands_id_from_help_argv(&argv(&[
                "extension",
                "commands",
                "-v",
                "weather-forecast-v2",
                "-h"
            ])),
            Some("weather-forecast-v2".into())
        );
        assert_eq!(
            extension_commands_id_from_help_argv(&argv(&["extension", "commands", "--help"])),
            None,
            "bare verb help has no id"
        );
        assert_eq!(
            extension_commands_id_from_help_argv(&argv(&["dashboard", "list", "--help"])),
            None
        );
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
        ] {
            let err = dispatch(&argv(&args)).await.unwrap_err();
            assert!(
                matches!(err, DispatchError::NotInProcess),
                "`neomind {:?}` must be NotInProcess, got {err:?}",
                args
            );
        }
    }

    /// S2-3: `health` runs in-process via API JSON (never NotInProcess /
    /// Tauri subprocess). Without a server it surfaces Api, not NotInProcess.
    #[tokio::test]
    async fn health_is_in_process_not_subprocess() {
        let err = dispatch(&argv(&["health"])).await.unwrap_err();
        assert!(
            !matches!(err, DispatchError::NotInProcess),
            "health must stay in-process, got {err:?}"
        );
        assert!(
            matches!(err, DispatchError::Api(_)),
            "without a server health should be Api(...), got {err:?}"
        );
    }

    /// Local-only commands that an in-process caller cannot serve also stay
    /// out of process: reset-password reads a hidden password from stdin
    /// (an agent-shell dispatch would hang on it), and extension
    /// create/build/validate manage local files via the full CLI's
    /// implementation. api-key and user list/set-role ARE in-process now
    /// (HTTP endpoint / auth redb mirror) — their dispatch must succeed or
    /// fail with a real error, never NotInProcess (the desktop `neomind`
    /// alias resolving to the tauri binary makes the subprocess fallback a
    /// silent GUI-exit-0 — the exact fake-success this split exists to kill).
    #[tokio::test]
    async fn local_only_commands_are_not_in_process() {
        for args in [
            vec!["user", "reset-password", "admin"],
            vec!["extension", "create", "x"],
        ] {
            let err = dispatch(&argv(&args)).await.unwrap_err();
            assert!(
                matches!(err, DispatchError::NotInProcess),
                "`neomind {:?}` must be NotInProcess, got {err:?}",
                args
            );
        }
    }

    /// `--help` (and `-V`) are clap DisplayHelp/Version "errors" — the
    /// in-process path must return them as SUCCESS with the rendered text,
    /// matching the real binary's exit-0 contract. Regression: the agent's
    /// `dashboard list --help` came back exit 2 with an "error:" prefix, so
    /// the model treated a working help call as a failure and retried it.
    #[tokio::test]
    async fn help_is_success_not_parse_error() {
        let resp = dispatch(&argv(&["dashboard", "list", "--help"]))
            .await
            .expect("--help must not be a Parse error");
        assert!(resp.success, "help is exit 0");
        let text = resp.data.expect("help payload")["help"]
            .as_str()
            .expect("rendered help text")
            .to_string();
        assert!(text.contains("Usage"), "help text must render: {text}");
        assert!(
            !text.starts_with("error:"),
            "no error prefix on help: {text}"
        );
    }

    /// Bare `extension commands --help` (no id) stays clap Usage — not a
    /// discovery redirect.
    #[tokio::test]
    async fn extension_commands_bare_help_stays_usage() {
        let resp = dispatch(&argv(&["extension", "commands", "--help"]))
            .await
            .expect("bare --help must succeed");
        assert!(resp.success);
        let help = resp
            .data
            .as_ref()
            .and_then(|d| d.get("help"))
            .and_then(|v| v.as_str());
        assert!(
            help.is_some_and(|h| h.contains("Usage")),
            "expected Usage help, got {:?}",
            resp.data
        );
        assert!(resp
            .data
            .as_ref()
            .and_then(|d| d.get("_help_redirect"))
            .is_none());
    }

    /// `extension commands <id> --help` must NOT return clap meta-help as if
    /// discovery completed (P0-A). Without a server the commands path may
    /// error — still must be the commands path, not Usage-only.
    #[serial_test::serial]
    #[tokio::test]
    async fn extension_commands_id_help_redirects_away_from_meta_help() {
        std::env::set_var("NEOMIND_API_BASE", "http://127.0.0.1:9/test-api");
        let result = dispatch(&argv(&[
            "extension",
            "commands",
            "weather-forecast-v2",
            "--help",
        ]))
        .await;
        std::env::remove_var("NEOMIND_API_BASE");

        let resp = match result {
            Ok(r) => r,
            Err(DispatchError::Api(_)) => return, // redirected into handler; API down is fine
            Err(other) => panic!("expected redirect into commands path, got {other:?}"),
        };
        let data = resp.data.as_ref().expect("redirect payload");
        assert!(
            data.get("help").is_none(),
            "must not return clap Usage as discovery: {data}"
        );
        assert_eq!(
            data.get("_help_redirect").and_then(|v| v.as_bool()),
            Some(true),
            "expected _help_redirect marker: {data}"
        );
        assert!(
            data.get("note")
                .and_then(|v| v.as_str())
                .is_some_and(|n| n.contains("no --help")),
            "expected redirect note: {data}"
        );
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
        let ctx = DispatchContext {
            api_base: Some("http://127.0.0.1:9/test-api".to_string()),
            api_key: None,
            output_format: Some(OutputFormat::Json),
        };
        let result = dispatch_with_context(&argv(&["device", "list"]), &ctx).await;

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

    async fn spawn_mock_http_server(
        responses: Vec<(&'static str, String)>,
    ) -> (
        std::net::SocketAddr,
        tokio::task::JoinHandle<Vec<(String, String)>>,
    ) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let mut captured = Vec::new();
            for (status_line, body) in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let header_end = loop {
                    let n = stream.read(&mut chunk).await.unwrap();
                    if n == 0 {
                        break buf.len();
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break pos + 4;
                    }
                };
                let headers_str = String::from_utf8_lossy(&buf[..header_end]).to_string();
                let content_len: usize = headers_str
                    .lines()
                    .find_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        if k.eq_ignore_ascii_case("content-length") {
                            v.trim().parse().ok()
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);
                while buf.len() < header_end + content_len {
                    let n = stream.read(&mut chunk).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let body_str =
                    String::from_utf8_lossy(&buf[header_end..header_end + content_len]).to_string();
                captured.push((headers_str, body_str));

                let reply = format!(
                    "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(reply.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
            captured
        });
        (addr, handle)
    }

    /// P1-1: concurrent callers with distinct `DispatchContext`s hit their own
    /// servers with their own Bearer keys without mutating process env vars.
    #[tokio::test]
    async fn dispatch_with_context_isolates_concurrent_callers_without_env_mutation() {
        let env_key_before = std::env::var_os("NEOMIND_API_KEY");
        let env_json_before = std::env::var_os("NEOMIND_JSON");

        let (addr_a, srv_a) = spawn_mock_http_server(vec![(
            "200 OK",
            r#"{"success":true,"data":{"id":"ext-a","name":"Ext A"}}"#.to_string(),
        )])
        .await;
        let (addr_b, srv_b) = spawn_mock_http_server(vec![(
            "200 OK",
            r#"{"success":true,"data":{"id":"ext-b","name":"Ext B"}}"#.to_string(),
        )])
        .await;

        let ctx_a = DispatchContext {
            api_base: Some(format!("http://{addr_a}/api")),
            api_key: Some("nmk_caller_a".to_string()),
            output_format: Some(OutputFormat::Json),
        };
        let ctx_b = DispatchContext {
            api_base: Some(format!("http://{addr_b}/api")),
            api_key: Some("nmk_caller_b".to_string()),
            output_format: Some(OutputFormat::Json),
        };

        let argv_a = argv(&["extension", "get", "ext-a"]);
        let argv_b = argv(&["extension", "get", "ext-b"]);
        let (res_a, res_b) = tokio::join!(
            dispatch_with_context(&argv_a, &ctx_a),
            dispatch_with_context(&argv_b, &ctx_b),
        );

        let resp_a = res_a.expect("caller A must succeed");
        let resp_b = res_b.expect("caller B must succeed");
        assert_eq!(resp_a.data.unwrap()["id"], "ext-a");
        assert_eq!(resp_b.data.unwrap()["id"], "ext-b");

        let reqs_a = srv_a.await.unwrap();
        let reqs_b = srv_b.await.unwrap();
        assert!(
            reqs_a[0].0.contains("authorization: Bearer nmk_caller_a")
                || reqs_a[0].0.contains("Authorization: Bearer nmk_caller_a"),
            "caller A header mismatch: {}",
            reqs_a[0].0
        );
        assert!(
            reqs_b[0].0.contains("authorization: Bearer nmk_caller_b")
                || reqs_b[0].0.contains("Authorization: Bearer nmk_caller_b"),
            "caller B header mismatch: {}",
            reqs_b[0].0
        );
        assert_eq!(
            std::env::var_os("NEOMIND_API_KEY"),
            env_key_before,
            "dispatch_with_context must never mutate NEOMIND_API_KEY"
        );
        assert_eq!(
            std::env::var_os("NEOMIND_JSON"),
            env_json_before,
            "dispatch_with_context must never mutate NEOMIND_JSON"
        );
    }

    /// P1-1: `extension get`, `extension install`, and `extension uninstall`
    /// unify onto the HTTP in-process path by default, while `--offline` and
    /// local dev commands (`validate`, `create`, `build`) stay `NotInProcess`.
    #[tokio::test]
    async fn extension_install_uninstall_get_use_http_and_offline_is_not_in_process() {
        let tmp_nep =
            std::env::temp_dir().join(format!("neomind_p1_1_test_{}.nep", std::process::id()));
        std::fs::write(&tmp_nep, b"PK\x03\x04dummy-nep-bytes").unwrap();
        let tmp_nep_str = tmp_nep.to_string_lossy().to_string();

        let (addr, srv) = spawn_mock_http_server(vec![
            (
                "200 OK",
                r#"{"success":true,"data":{"id":"weather-v2","version":"1.0.0"}}"#.to_string(),
            ),
            (
                "200 OK",
                r#"{"success":true,"data":{"extension_id":"weather-v2","version":"1.0.0"}}"#
                    .to_string(),
            ),
            (
                "200 OK",
                r#"{"success":true,"data":{"extension_id":"weather-v2","message":"Extension uninstalled completely"}}"#
                    .to_string(),
            ),
        ])
        .await;

        let ctx = DispatchContext {
            api_base: Some(format!("http://{addr}/api")),
            api_key: Some("nmk_ext_test".to_string()),
            output_format: Some(OutputFormat::Json),
        };

        let get_resp = dispatch_with_context(&argv(&["extension", "get", "weather-v2"]), &ctx)
            .await
            .expect("extension get must run in-process via HTTP");
        assert!(get_resp.success);
        assert_eq!(get_resp.data.unwrap()["id"], "weather-v2");

        let install_resp =
            dispatch_with_context(&argv(&["extension", "install", &tmp_nep_str]), &ctx)
                .await
                .expect("extension install must run in-process via HTTP upload");
        assert!(install_resp.success);
        assert_eq!(install_resp.data.unwrap()["extension_id"], "weather-v2");

        let uninstall_resp = dispatch_with_context(
            &argv(&["extension", "uninstall", "weather-v2", "--yes"]),
            &ctx,
        )
        .await
        .expect("extension uninstall must run in-process via HTTP");
        assert!(uninstall_resp.success);

        let _ = std::fs::remove_file(&tmp_nep);

        let reqs = srv.await.unwrap();
        assert_eq!(reqs.len(), 3);
        assert!(reqs[0].0.starts_with("GET /api/extensions/weather-v2 "));
        assert!(reqs[1].0.starts_with("POST /api/extensions/upload/file "));
        let upload_body: serde_json::Value = serde_json::from_str(&reqs[1].1).unwrap();
        assert!(upload_body["data"].as_str().is_some_and(|s| !s.is_empty()));
        assert!(upload_body["filename"]
            .as_str()
            .is_some_and(|f| f.ends_with(".nep")));
        assert!(reqs[2]
            .0
            .starts_with("DELETE /api/extensions/weather-v2/uninstall "));

        // Offline mode and true local dev commands must remain NotInProcess.
        for local_args in [
            vec!["extension", "get", "weather-v2", "--offline"],
            vec!["extension", "install", "./foo.nep", "--offline"],
            vec!["extension", "uninstall", "weather-v2", "--offline"],
            vec!["extension", "validate", "./foo.nep"],
            vec!["extension", "create", "my-ext"],
            vec!["extension", "build", "./my-ext"],
        ] {
            let err = dispatch_with_context(&argv(&local_args), &ctx)
                .await
                .unwrap_err();
            assert!(
                matches!(err, DispatchError::NotInProcess),
                "`neomind {:?}` must stay NotInProcess, got {err:?}",
                local_args
            );
        }
    }
}
