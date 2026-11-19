//! Shell tool for executing system commands.
//!
//! Allows the AI agent to run arbitrary shell commands on the host system.
//! Cross-platform: uses `/bin/sh -c` on Unix, `cmd /C` on Windows.
//! Disabled by default — must be explicitly enabled in agent configuration.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;

use neomind_core::tools::ToolCategory;

use super::error::{Result, ToolError};
use super::tool::{object_schema, Tool, ToolOutput};

/// Shell tool configuration, stored as part of agent config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellConfig {
    /// Whether shell tool is enabled. Default: false.
    #[serde(default)]
    pub enabled: bool,

    /// Maximum execution time per command in seconds. Default: 30.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Maximum output characters (stdout + stderr combined). Default: 10000.
    #[serde(default = "default_max_output")]
    pub max_output_chars: usize,
}

fn default_timeout() -> u64 {
    crate::toolkit::timeouts::shell_default().as_secs()
}

fn default_max_output() -> usize {
    10000
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_secs: default_timeout(),
            max_output_chars: default_max_output(),
        }
    }
}

/// Output from a shell command execution.
#[derive(Debug)]
struct CommandOutput {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    timed_out: bool,
    /// Cross-domain CLI index card emitted once per session as a separate JSON
    /// field (never concatenated onto `stdout`).
    domain_index: Option<String>,
    /// In-process clap `--help` reference emitted ONLY on clap parse failure /
    /// unknown subcommand as a separate JSON field (never concatenated onto
    /// `stdout` or `stderr`).
    usage_hint: Option<String>,
}

impl CommandOutput {
    fn plain(exit_code: Option<i32>, stdout: String, stderr: String, timed_out: bool) -> Self {
        Self {
            exit_code,
            stdout,
            stderr,
            timed_out,
            domain_index: None,
            usage_hint: None,
        }
    }
}

/// Cross-domain subcommand index (channel D). Delivered ONCE per session in
/// the separate `domain_index` field of the first in-process `neomind` shell
/// tool result (never appended to `stdout`, so `stdout` stays 100% valid JSON).
///
/// History: first shipped inside the static description (2026-08-17); the
/// 2026-08-18 full eval showed the 2600-char description re-triggered the
/// ≤3B tool-selection suppression (wrong-tool grabs like file_write), so it
/// moved here. The 27 "detoured via a wrong-but-succeeding command" failures
/// it fixes need the map BEFORE detouring, which channel D satisfies for
/// every session that touches shell at least once.
const DOMAIN_INDEX: &str = r#"

[neomind CLI domain index — the exact subcommands that exist (one line per domain; `neomind <domain> --help` for flags)]
- device: list get create update delete history latest control <ID> <CMD> types write-metric webhook-url drafts (write-metric writes/simulates a data point — the way to feed a virtual-device test report; no broker or connection involved)
- agent: list get create update delete control <ID> <active|paused> invoke memory clear-memory executions <ID> latest-execution conversation <ID> send-message <ID> (talking to an agent, NOT `message`)
- rule: list get create update delete enable disable test history explain
- dashboard: list get create update delete add-components update-component remove-components share (ADD widgets → add-components (append); TWEAK one widget → update-component; `update --components` replaces ALL and needs --replace-all — almost never what you want; `dashboard get <ID>` first to see layout/ids)
- connector: list get create update delete enable disable test subscribe subscriptions unsubscribe (external MQTT data ingestion / subscriptions; outbound webhook or MQTT data targets use push)
- extension: list get install uninstall status logs config reload create build market-list market-install validate commands invoke
- transform: list get create update delete enable disable metrics test-code data-sources executions (executions = recent run records; check it when a transform outputs nothing or fails)
- widget: list get create install uninstall bundle market-list market-install
- message: list get send read delete channel-list channel-get channel-types channel-type-schema channel-create channel-update channel-delete channel-test (`message send` creates a platform message — NOT for talking to agents; to act on any device use `device get <id>` then `device control <id> <command>`)
- push: list get create update delete enable disable start stop test logs stats
- llm: list get models create update delete activate test
- settings: timezone set-timezone timezones retention set-retention cleanup
- operation: list get (inspect persisted write-operation ledger status by operation_id; check whether an unknown/timed-out operation succeeded or failed)
- system: info
- api-key: create list delete
Deliberately not available here (do not try them): serve prompt chat logs health list-models
check-update upgrade uninstall login logout whoami user config data — the first group only runs as a
separate process, the rest are operator commands with their own session or irreversible effects.

This is a common cheat sheet of domain verbs; full flags and aliases come from `neomind <domain> --help` (or `neomind <domain> <command> --help`). Do not invent near-miss subcommands — use an exact listed name or discover via `--help`."#;

pub struct ShellTool {
    config: ShellConfig,
    /// Session-scoped flag: set to `true` once `DOMAIN_INDEX` has been emitted
    /// for this chat session / tool instance. Replaces the former process-global
    /// `INDEX_INJECTED` static so Session A and Session B in a long-running
    /// server receive identical domain-index injection.
    index_injected: std::sync::atomic::AtomicBool,
}

impl ShellTool {
    fn command_timeout(&self, args: &Value) -> Result<Duration> {
        // Resolve timeout: per-command override or config default, capped at 600s
        // Accepts both number and string forms (LLM may pass "30" as string)
        let timeout = if let Some(user_timeout) = args.get("timeout") {
            let secs = user_timeout
                .as_u64()
                .or_else(|| user_timeout.as_str().and_then(|s| s.parse::<u64>().ok()))
                .ok_or_else(|| {
                    ToolError::InvalidArguments("timeout must be a positive number".into())
                })?;
            Duration::from_secs(secs.min(crate::toolkit::timeouts::shell_max().as_secs()))
        } else {
            Duration::from_secs(
                self.config
                    .timeout_secs
                    .min(crate::toolkit::timeouts::shell_max().as_secs()),
            )
        };

        Ok(timeout)
    }

    pub fn new(config: ShellConfig) -> Self {
        Self {
            config,
            index_injected: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Create an isolated `ShellTool` for a single chat session with fresh
    /// per-session injection state.
    pub(crate) fn for_chat_session(&self, _session_id: &str) -> Self {
        Self::new(self.config.clone())
    }

    /// Emit the cross-domain index card once per session.
    fn take_domain_index_once(&self) -> Option<String> {
        if !self
            .index_injected
            .swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            Some(DOMAIN_INDEX.trim().to_string())
        } else {
            None
        }
    }

    /// Build a platform-appropriate shell command.
    /// Unix: login shell (`$SHELL -l -c`) with isolated process group;
    ///       falls back to `/bin/sh -c` without `-l` if $SHELL is not set.
    /// Windows: `cmd /C`
    fn build_command(command: &str) -> std::process::Command {
        let (shell, is_login) = shell_path();
        let mut cmd = std::process::Command::new(shell);
        shell_arg(&mut cmd, command, is_login);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        set_process_group(&mut cmd);

        // Inject NEOMIND_API_KEY so spawned neomind CLI can authenticate
        // without depending on CWD-relative data/api_keys.redb lookup.
        if let Some(key) = Self::resolve_api_key() {
            cmd.env("NEOMIND_API_KEY", key);
        }

        // Force JSON output — the AI agent is a machine consumer that needs
        // structured data. Without this, CLI defaults to human-readable format
        // which strips most useful information from the output.
        cmd.env("NEOMIND_JSON", "1");

        // Prepend a directory containing a `neomind` that resolves to the
        // binary currently running the server, so subprocess `neomind ...`
        // invocations can't hit a stale install (`~/.cargo/bin/neomind` was
        // 0.9.24 while the platform ran 1.1.2 — silent version AND data-dir
        // drift, particularly for local-only commands that bypass in-process
        // dispatch). When this binary IS `neomind`, its own directory is
        // that directory. The desktop bundle names it differently
        // (`neomind-tauri`), so a plain PATH entry can never satisfy a
        // `neomind` lookup — alias it under <data>/bin/ and prepend THAT.
        if let Ok(exe) = std::env::current_exe() {
            let is_plain_neomind = exe
                .file_name()
                .map(|f| f == "neomind" || f == "neomind.exe")
                .unwrap_or(false);
            let dir_to_prepend = if is_plain_neomind {
                exe.parent().map(|p| p.to_path_buf())
            } else {
                Self::ensure_neomind_alias(&exe)
            };
            if let Some(dir) = dir_to_prepend {
                let dir = dir.display().to_string();
                match std::env::var_os("PATH") {
                    Some(existing) => {
                        let new_path =
                            format!("{}{}{}", dir, path_delimiter(), existing.to_string_lossy());
                        cmd.env("PATH", new_path);
                    }
                    None => {
                        cmd.env("PATH", dir);
                    }
                }
            }
        }

        cmd
    }

    /// Create/refresh `<data>/bin/neomind` pointing at `exe` (the running
    /// binary, whatever it's named) and return the directory to prepend to
    /// PATH. Best-effort: any failure returns None (PATH stays untouched —
    /// the pre-fix behavior, no worse).
    ///
    /// Unix: symlink (cheap, refresh every call so a bundle update is
    /// picked up on the next tool call). Windows: symlinks need privileges;
    /// copy instead, guarded by size so we only re-copy when the binary
    /// actually changed.
    fn ensure_neomind_alias(exe: &std::path::Path) -> Option<std::path::PathBuf> {
        let dir = neomind_core::paths::data_dir().join("bin");
        std::fs::create_dir_all(&dir).ok()?;
        let alias = dir.join(if cfg!(windows) {
            "neomind.exe"
        } else {
            "neomind"
        });
        #[cfg(unix)]
        {
            // remove-then-symlink: handles both stale target and first run.
            let _ = std::fs::remove_file(&alias);
            std::os::unix::fs::symlink(exe, &alias).ok()?;
        }
        #[cfg(windows)]
        {
            let alias_len = std::fs::metadata(&alias).map(|m| m.len()).ok();
            let exe_len = std::fs::metadata(exe).map(|m| m.len()).ok();
            let need_copy = match (alias_len, exe_len) {
                (a, Some(e)) => a != Some(e),
                // Exe unreadable (shouldn't happen — it's running) or first
                // run: attempt the copy; fs::copy's own error decides.
                _ => true,
            };
            if need_copy {
                std::fs::copy(exe, &alias).ok()?;
            }
        }
        Some(dir)
    }

    /// Resolve API key for neomind CLI commands.
    ///
    /// Checks env var first, then reads directly from the server's redb.
    /// Deliberately skips the credential file layer (`read_default_api_key`)
    /// because the agent runs inside the server process — it should use the
    /// server's own key, not a credential file that may have been written by
    /// `neomind login` against a different server instance.
    fn resolve_api_key() -> Option<String> {
        std::env::var("NEOMIND_API_KEY").ok().or_else(|| {
            neomind_cli_ops::auto_auth::read_default_api_key_from(
                &neomind_cli_ops::auto_auth::resolve_data_dir(),
            )
        })
    }

    /// Render compact in-process `--help` directly from clap's
    /// `Args::command()` tree (zero subprocess spawn, no `current_exe()`
    /// hazard in desktop/Tauri binaries).
    ///
    /// Attached ONLY to the separate `usage_hint` JSON field on clap parse
    /// failure / unknown subcommand — never appended to `stdout` and never
    /// emitted on successful commands.
    fn in_process_domain_help(
        domain: &str,
        subcommand: Option<&str>,
        note: &str,
    ) -> Option<String> {
        use clap::CommandFactory;

        if domain.is_empty() {
            return None;
        }
        let mut root = neomind_cli_ops::dispatch::commands::Args::command();
        root.build();

        let domain_cmd = root
            .get_subcommands_mut()
            .find(|c| c.get_name() == domain || c.get_all_aliases().any(|a| a == domain))?;

        // Prefer the SUBCOMMAND's help — it lists the exact flags/args the
        // model guessed wrong (`device list --all`, `rule create --id=...`,
        // `dashboard update-component --bad-flag`).
        if let Some(sub) = subcommand.filter(|s| !s.starts_with('-')) {
            if let Some(sub_cmd) = domain_cmd
                .get_subcommands_mut()
                .find(|c| c.get_name() == sub || c.get_all_aliases().any(|a| a == sub))
            {
                let body: String = sub_cmd
                    .render_help()
                    .to_string()
                    .lines()
                    .take(28)
                    .collect::<Vec<_>>()
                    .join("\n");
                if !body.trim().is_empty() {
                    return Some(format!(
                        "[Reference — `neomind {} {}` — {}]:\n{}",
                        domain, sub, note, body
                    ));
                }
            }
        }

        // Fallback: bare domain help (lists available subcommands).
        let body: String = domain_cmd
            .render_help()
            .to_string()
            .lines()
            .take(28)
            .collect::<Vec<_>>()
            .join("\n");
        if body.trim().is_empty() {
            return None;
        }
        Some(format!(
            "[Reference — `neomind {}` subcommands — {}]:\n{}",
            domain, note, body
        ))
    }

    /// Resolve the `neomind` binary for argv-based subprocess spawn
    /// (no shell). Prefers the running server binary / data-dir alias so
    /// PATH drift cannot reintroduce a stale CLI.
    fn resolve_neomind_executable() -> std::path::PathBuf {
        if let Ok(exe) = std::env::current_exe() {
            let is_plain_neomind = exe
                .file_name()
                .map(|f| f == "neomind" || f == "neomind.exe")
                .unwrap_or(false);
            if is_plain_neomind {
                return exe;
            }
            if let Some(dir) = Self::ensure_neomind_alias(&exe) {
                return dir.join(if cfg!(windows) {
                    "neomind.exe"
                } else {
                    "neomind"
                });
            }
        }
        std::path::PathBuf::from("neomind")
    }

    /// Spawn `argv` (already tokenized `neomind …`) without `/bin/sh -c`.
    ///
    /// Used when in-process dispatch returns [`DispatchError::NotInProcess`]:
    /// falling back to the raw shell string would re-expand quoted `$()` /
    /// backticks that tokenize intentionally kept literal — including after a
    /// ProvenReadOnly classify (e.g. `list-models`).
    async fn spawn_neomind_argv(&self, argv: &[String], timeout: Duration) -> CommandOutput {
        let exe = Self::resolve_neomind_executable();
        let mut cmd = std::process::Command::new(&exe);
        if argv.len() > 1 {
            cmd.args(&argv[1..]);
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        set_process_group(&mut cmd);
        if let Some(key) = Self::resolve_api_key() {
            cmd.env("NEOMIND_API_KEY", key);
        }
        cmd.env("NEOMIND_JSON", "1");

        tracing::debug!(
            target: "neomind::agent::shell",
            in_process = false,
            argv = ?argv,
            exe = %exe.display(),
            "NotInProcess — spawning parsed argv without shell reinterpretation"
        );

        let mut child = match tokio::process::Command::from(cmd).spawn() {
            Ok(c) => c,
            Err(e) => {
                return CommandOutput::plain(
                    Some(1),
                    String::new(),
                    format!("Failed to spawn neomind argv: {e}"),
                    false,
                );
            }
        };
        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();
        let mut guard = SubprocessGuard { child: Some(child) };
        let result = tokio::time::timeout(timeout, async {
            let stdout_fut = async {
                if let Some(mut s) = stdout_handle {
                    let mut buf = Vec::new();
                    s.read_to_end(&mut buf).await?;
                    Ok::<_, std::io::Error>(buf)
                } else {
                    Ok(Vec::new())
                }
            };
            let stderr_fut = async {
                if let Some(mut s) = stderr_handle {
                    let mut buf = Vec::new();
                    s.read_to_end(&mut buf).await?;
                    Ok::<_, std::io::Error>(buf)
                } else {
                    Ok(Vec::new())
                }
            };
            let (out_bytes, err_bytes) = tokio::try_join!(stdout_fut, stderr_fut)?;
            let status = guard
                .child
                .as_mut()
                .ok_or_else(|| std::io::Error::other("child disarmed before wait"))?
                .wait()
                .await?;
            Ok::<_, std::io::Error>((out_bytes, err_bytes, status))
        })
        .await;

        match result {
            Ok(Ok((out, err, status))) => {
                guard.child = None;
                let raw_stdout = String::from_utf8_lossy(&out).into_owned();
                let raw_stderr = String::from_utf8_lossy(&err).into_owned();
                let (stdout, stderr) =
                    truncate_output(&raw_stdout, &raw_stderr, self.config.max_output_chars);
                CommandOutput::plain(status.code(), stdout, stderr, false)
            }
            Ok(Err(e)) => {
                guard.child = None;
                CommandOutput::plain(
                    Some(1),
                    String::new(),
                    format!("argv spawn I/O error: {e}"),
                    false,
                )
            }
            Err(_) => CommandOutput::plain(
                None,
                String::new(),
                format!("Command timed out after {}s", timeout.as_secs()),
                true,
            ),
        }
    }

    /// Attempt in-process dispatch for `neomind` data commands.
    ///
    /// Returns `Some(output)` if the command was handled in-process (success,
    /// parse/API error) **or** via argv subprocess after
    /// [`DispatchError::NotInProcess`]. Returns `None` only when the line
    /// cannot be tokenized as pure `neomind` argv — then the caller may use
    /// the shell path.
    ///
    /// Non-`neomind` commands and malformed input (unbalanced quotes) also
    /// yield `None` so they hit the subprocess path unchanged.
    async fn try_in_process_dispatch(
        &self,
        command: &str,
        timeout: Duration,
    ) -> Option<CommandOutput> {
        // Truncation pipelines (`neomind device list 2>&1 | head -100`) are
        // applied in-process: dispatch the base command, then cut the output.
        // Without this the pipes fall back to a subprocess whose CLI goes
        // through the HTTP API — an auth dependency pure data queries don't
        // need. Unsupported stages (grep/sort/…) return None → subprocess.
        let (base, merge_stderr, truncation) = split_truncation_pipeline(command.trim())?;
        let output = self.dispatch_in_process(&base, timeout).await?;
        Some(apply_truncation_pipeline(output, merge_stderr, &truncation))
    }

    /// In-process dispatch of a single (pipe-free) neomind command line.
    async fn dispatch_in_process(&self, command: &str, timeout: Duration) -> Option<CommandOutput> {
        let trimmed = command.trim();

        // Shell sequencing: `&&` (stop on failure) or `;` (always continue).
        // The agent naturally batches, e.g. `neomind device get a; neomind
        // device get b` or `neomind system info; echo "---"; neomind device list`.
        // Handle in-process when ALL parts are neomind commands; mixed
        // neomind/non-neomind (e.g. with echo) falls through to subprocess.
        let has_sep = trimmed.contains("&&") || trimmed.contains(";");
        if has_sep {
            // Pick the separator that appears first to split on.
            let sep = if let Some(amp) = trimmed.find("&&") {
                if let Some(semi) = trimmed.find(';') {
                    if semi < amp {
                        ";"
                    } else {
                        "&&"
                    }
                } else {
                    "&&"
                }
            } else {
                ";"
            };
            let parts: Vec<&str> = trimmed
                .split(sep)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();
            let all_neomind = parts.len() > 1
                && parts
                    .iter()
                    .all(|p| p.starts_with("neomind ") || *p == "neomind");
            if all_neomind {
                let mut outputs: Vec<String> = Vec::new();
                let mut domain_index: Option<String> = None;
                let mut usage_hint: Option<String> = None;
                for part in parts {
                    // Boxed: the recursive async call needs indirection (the
                    // chain length is runtime-variable → unbounded future).
                    match Box::pin(self.try_in_process_dispatch(part, timeout)).await {
                        Some(out) => {
                            let code = out.exit_code.unwrap_or(1);
                            outputs.push(out.stdout);
                            if domain_index.is_none() {
                                domain_index = out.domain_index;
                            }
                            if out.usage_hint.is_some() {
                                usage_hint = out.usage_hint;
                            }
                            // `&&` stops on failure; `;` always continues.
                            if sep == "&&" && code != 0 {
                                return Some(CommandOutput {
                                    exit_code: Some(code),
                                    stdout: outputs.join("\n--- && ---\n"),
                                    stderr: out.stderr,
                                    timed_out: false,
                                    domain_index,
                                    usage_hint,
                                });
                            }
                        }
                        None => return None, // a part isn't in-processable → subprocess
                    }
                }
                return Some(CommandOutput {
                    exit_code: Some(0),
                    stdout: outputs.join(format!("\n--- {} ---\n", sep.trim()).as_str()),
                    stderr: String::new(),
                    timed_out: false,
                    domain_index,
                    usage_hint,
                });
            }
            // Mixed neomind/non-neomind with && or ; → let /bin/sh handle it.
            return None;
        }

        // Only intercept commands that start with `neomind ` (or are exactly
        // `neomind`). Anything else goes to the subprocess path.
        if !trimmed.starts_with("neomind ") && trimmed != "neomind" {
            return None;
        }

        // Tokenize. `neomind` data commands are simple enough that a basic
        // quote-respecting whitespace split is sufficient. We do NOT need
        // full shell syntax (pipes / redirections / $ expansions) because
        // those constructs are never part of a pure data query — they'd hit
        // the subprocess path by design.
        let argv = match tokenize_neomind_command(trimmed) {
            Ok(v) => v,
            Err(e) => {
                tracing::debug!(
                    target: "neomind::agent::shell",
                    in_process = false,
                    command = %command,
                    reason = %e,
                    "in-process dispatch skipped (tokenize error)"
                );
                return None;
            }
        };

        if argv.is_empty() || argv[0] != "neomind" {
            return None;
        }

        // Host lifecycle tokens must never reach clap (`_new_operation=true`
        // appended inside `command` was a live clap "unexpected argument").
        let argv = strip_host_meta_from_argv(argv);

        // Structured shell `params` → primary JSON flag as a clean argv token
        // (no model-facing quote wrapping). Host context wins when present.
        let argv = match inject_structured_json_argv(argv) {
            Ok(a) => a,
            Err(msg) => {
                return Some(CommandOutput::plain(Some(1), String::new(), msg, false));
            }
        };

        // NM-240: bind Chat write-gate to the same PreparedInvoke HTTP will
        // consume. Authorize Mutating tickets on the execute path (RO was
        // authorized at classify); inject --prepared-operation-id.
        let argv = match bind_extension_invoke_prepared_token(argv) {
            Ok(a) => a,
            Err(msg) => {
                return Some(CommandOutput::plain(Some(1), String::new(), msg, false));
            }
        };

        // Pass auth + JSON output explicitly via DispatchContext — never mutate
        // process-global environment variables (`std::env::set_var`) on an
        // async multi-threaded hot path.
        let dispatch_ctx = neomind_cli_ops::dispatch::DispatchContext {
            api_base: std::env::var("NEOMIND_API_BASE").ok(),
            api_key: Self::resolve_api_key(),
            output_format: Some(neomind_cli_ops::types::OutputFormat::Json),
        };

        tracing::debug!(
            target: "neomind::agent::shell",
            in_process = true,
            command = %command,
            "dispatching neomind command in-process"
        );

        // Apply the same per-command timeout as the subprocess path. The
        // ApiClient has its own HTTP timeout (70s for agent invoke), but may issue
        // multiple requests; this guarantees the in-process path cannot hang
        // longer than the subprocess equivalent would.
        match tokio::time::timeout(
            timeout,
            neomind_cli_ops::dispatch::dispatch_with_context(&argv, &dispatch_ctx),
        )
        .await
        {
            Ok(Ok(resp)) => {
                let exit_code = if resp.success { 0 } else { 1 };
                let stdout = serde_json::to_string_pretty(&resp).unwrap_or_else(|e| {
                    format!(
                        "{{\"success\":false,\"error\":\"serialize failed: {}\"}}",
                        e
                    )
                });
                // Keep `stdout` 100% valid JSON: deliver the session-scoped
                // cross-domain index card in its own `domain_index` field once
                // per session, and never attach `--help` on successful runs or
                // runtime business errors.
                let domain_index = self.take_domain_index_once();
                Some(CommandOutput {
                    exit_code: Some(exit_code),
                    stdout,
                    stderr: String::new(),
                    timed_out: false,
                    domain_index,
                    usage_hint: None,
                })
            }
            Ok(Err(neomind_cli_ops::dispatch::DispatchError::NotInProcess)) => {
                // Already have argv from tokenize — never re-interpret the raw
                // shell string (quoted `$()` would expand under /bin/sh -c).
                // Classifier may have marked this ProvenReadOnly (list-models /
                // health / whoami); argv spawn preserves that literal argv.
                debug_assert!(
                    not_in_process_uses_parsed_argv(),
                    "NotInProcess must spawn parsed argv, not raw shell"
                );
                Some(self.spawn_neomind_argv(&argv, timeout).await)
            }
            Ok(Err(neomind_cli_ops::dispatch::DispatchError::Parse(msg))) => {
                let mut stderr = parse_error_stderr(&msg);
                // A rejected flag needs the CLI usage, not an unrelated path
                // quoting hint. Inspect the offending token and its adjacent
                // argv fragment instead of quotes anywhere in the command.
                if should_hint_split_path(&msg, &argv) {
                    stderr.push_str(
                        "\nHint: an argument containing SPACES (e.g. a path under \"Application Support\") must be wrapped in double quotes: neomind widget install \"/Users/.../Application Support/.../device-info-widget\".",
                    );
                }
                let domain_index = self.take_domain_index_once();
                // Clap parse failure / unknown subcommand → attach in-process
                // clap `--help` in its own `usage_hint` JSON field (never
                // concatenated onto `stdout` or `stderr`). Preserve exit_code 2
                // so Phase 1's pre-submission rejection classifier matches.
                let usage_hint = argv.get(1).and_then(|domain| {
                    let subcommand = argv
                        .get(2)
                        .map(String::as_str)
                        .filter(|s| !s.starts_with('-'));
                    Self::in_process_domain_help(
                        domain.as_str(),
                        subcommand,
                        "retry with the exact one",
                    )
                    .or_else(|| {
                        if domain_index.is_none() {
                            Some(format!(
                                "Unknown domain `{}`. Valid domains: device, agent, rule, dashboard, connector, extension, transform, widget, message, push, llm, settings, system, api-key.",
                                domain
                            ))
                        } else {
                            None
                        }
                    })
                });
                Some(CommandOutput {
                    exit_code: Some(2),
                    stdout: String::new(),
                    stderr,
                    timed_out: false,
                    domain_index,
                    usage_hint,
                })
            }
            Ok(Err(neomind_cli_ops::dispatch::DispatchError::Api(msg))) => {
                let domain_index = self.take_domain_index_once();
                Some(CommandOutput {
                    exit_code: Some(1),
                    stdout: String::new(),
                    stderr: classify_extension_transport_error(&msg),
                    timed_out: false,
                    domain_index,
                    usage_hint: None,
                })
            }
            // Timeout (outer Err = elapsed) — mirror the subprocess timeout behavior.
            Err(_) => Some(CommandOutput::plain(
                None,
                String::new(),
                format!("Command timed out after {}s", timeout.as_secs()),
                true,
            )),
        }
    }

    /// Execute a command with timeout and output capture.
    async fn execute_command(
        &self,
        command: &str,
        working_dir: Option<&str>,
        timeout: Duration,
    ) -> Result<CommandOutput> {
        // [fabricated-output guard] A small model asked to install an
        // extension once answered by ECHOING a fake tool-result JSON
        // ("exit_code":0 ... "command":"neomind extension install X" ...
        // "message":"Installed extension") instead of running the real
        // command (2026-10-15) — the user was told the install succeeded
        // when nothing happened. Printing a tool-result-shaped payload via
        // echo/printf is never legitimate agent behavior: refuse it and
        // name the cure.
        if let Some(real_cmd) = fabricated_tool_output(command) {
            return Ok(CommandOutput::plain(
                Some(126),
                format!(
                    "FABRICATED TOOL OUTPUT REFUSED: this command prints a fake tool-result \
                     JSON (exit_code/command/stdout keys) instead of executing the real \
                     action — that reports success for something that never ran. Execute the \
                     real command instead: `{real_cmd}`"
                ),
                String::new(),
                false,
            ));
        }

        // Fast path: route `neomind` data commands through the in-process
        // dispatcher so the agent gets structured `CliResponse` directly,
        // without depending on whatever `neomind` binary happens to be in
        // PATH (eliminates version drift between the running server and the
        // CLI binary). `NotInProcess` (list-models/logs/…) spawns the already
        // parsed argv — never re-shells the raw string (quoted `$()`).
        if let Some(output) = self.try_in_process_dispatch(command, timeout).await {
            return Ok(output);
        }

        // Detect when the agent wraps `neomind` inside a script (python/bash/etc).
        // This breaks the $cached mechanism: the script captures neomind's output
        // internally and only prints metadata, so the LargeDataCache never sees
        // the full payload (images, large JSON). Inject a hint telling the agent
        // to call `neomind` directly so $cached works.
        let wrapped_neomind_hint = if !command.trim().starts_with("neomind ")
            && command.contains("neomind ")
            && (command.contains("python") || command.contains("bash") || command.contains("sh "))
        {
            Some(
                "\n\n[Hint: You are calling `neomind` through a script wrapper. \
                 When called directly (shell(command=\"neomind device get <id>\")), \
                 large payloads like images are automatically cached as $cached references \
                 that can be passed directly to vision(image=\"$cached:...\"). \
                 Script wrappers break this — the image data is lost. \
                 Try calling neomind directly next time.]"
                    .to_string(),
            )
        } else {
            None
        };

        let mut cmd = Self::build_command(command);

        if let Some(dir) = working_dir {
            let path = std::path::Path::new(dir);
            if !path.exists() {
                return Err(ToolError::Execution(format!(
                    "Working directory does not exist: {}",
                    dir
                )));
            }
            if !path.is_dir() {
                return Err(ToolError::Execution(format!(
                    "Path is not a directory: {}",
                    dir
                )));
            }
            cmd.current_dir(dir);
        }

        let mut child = tokio::process::Command::from(cmd)
            .spawn()
            .map_err(|e| ToolError::Execution(format!("Failed to spawn: {}", e)))?;

        // Take stdout/stderr pipes BEFORE the timeout race so the guard can
        // hold the `Child` independently. This is the key change from the
        // previous `wait_with_output`-based flow: that helper consumed the
        // Child, forcing the guard to hold only the PID (and exposing us to
        // PID recycling — kill the wrong process after the kernel reuses the
        // id). Holding the Child itself makes the kernel track ownership for
        // us: the PID stays associated with this handle until we drop it.
        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();

        // B3 fix: guard holds the Child (not just PID). Drop fires killpg
        // on the child's PID, which is guaranteed to still refer to OUR
        // process because the Child handle owns that PID slot in tokio's
        // process table.
        let mut guard = SubprocessGuard { child: Some(child) };

        let result = tokio::time::timeout(timeout, async {
            // Read stdout/stderr concurrently with wait(). Both pipes
            // were taken above, so they live independently of the Child.
            let stdout_fut = async {
                if let Some(mut s) = stdout_handle {
                    let mut buf = Vec::new();
                    s.read_to_end(&mut buf).await?;
                    Ok::<_, std::io::Error>(buf)
                } else {
                    Ok(Vec::new())
                }
            };
            let stderr_fut = async {
                if let Some(mut s) = stderr_handle {
                    let mut buf = Vec::new();
                    s.read_to_end(&mut buf).await?;
                    Ok::<_, std::io::Error>(buf)
                } else {
                    Ok(Vec::new())
                }
            };
            let (out_bytes, err_bytes) = tokio::try_join!(stdout_fut, stderr_fut)?;
            let status = guard
                .child
                .as_mut()
                .ok_or_else(|| std::io::Error::other("child disarmed before wait"))?
                .wait()
                .await?;
            Ok::<_, std::io::Error>((out_bytes, err_bytes, status))
        })
        .await;

        match result {
            Ok(Ok((out, err, status))) => {
                // Clean exit — disarm the guard so its Drop doesn't kill
                // an already-exited process group (would be a benign ESRCH
                // but disarming makes the intent obvious).
                guard.child = None;
                let raw_stdout = String::from_utf8_lossy(&out).into_owned();
                let raw_stderr = String::from_utf8_lossy(&err).into_owned();
                // Truncate SUBPROCESS output here (not in execute()) so that
                // in-process `neomind` output — returned unchanged by
                // try_in_process_dispatch above — reaches the streaming slim
                // layer intact. Subprocess stdout is arbitrary host output
                // (logs, file dumps) with no downstream size guard, so the
                // configured char cap still applies; in-process output's
                // large payloads are images/base64 the slim layer caches as
                // `$cached` refs, and the old 10k-char cap destroyed those
                // bytes before slim could cache them.
                let (stdout, stderr) =
                    truncate_output(&raw_stdout, &raw_stderr, self.config.max_output_chars);
                // Append the wrapped-neomind hint (if any) so the agent sees
                // it in the tool result and adjusts its next call.
                let stdout = if let Some(hint) = &wrapped_neomind_hint {
                    format!("{}{}", stdout, hint)
                } else {
                    stdout
                };
                Ok(CommandOutput::plain(status.code(), stdout, stderr, false))
            }
            Ok(Err(e)) => {
                // Pipe/wait error — let guard's Drop handle cleanup so we
                // don't try to await a possibly-broken Child.
                Err(ToolError::Execution(format!("Execution failed: {}", e)))
            }
            Err(_) => {
                // Timeout — explicitly reap so we don't leak a zombie. The
                // guard's Drop will then be a no-op (child is None).
                if let Some(child) = guard.child.as_mut() {
                    // Best-effort kill + reap. kill_process_by_pid sends
                    // killpg(SIGKILL) which terminates the whole group;
                    // child.wait() reaps the immediate child.
                    if let Some(pid) = child.id() {
                        kill_process_by_pid(Some(pid));
                    }
                    let _ = child.wait().await;
                }
                guard.child = None;
                Ok(CommandOutput::plain(
                    None,
                    String::new(),
                    format!("Command timed out after {}s", timeout.as_secs()),
                    true,
                ))
            }
        }
    }
}

/// RAII guard that kills a subprocess (and its process group on Unix) when
/// dropped. Used to guarantee cleanup when the future returned by
/// `ShellTool::execute_command` is dropped before completion — the exact path
/// taken when a `CancellationToken` fires and the ToolRegistry `select!`
/// cancels the tool future.
///
/// B3 fix: holds the actual `Child` handle, NOT just the PID. This makes the
/// kernel track PID ownership — the PID cannot be recycled to a different
/// process until we drop this handle. The earlier PID-only design (used
/// because `wait_with_output` consumed the Child) had a small but real
/// risk of killing an unrelated process after PID recycling.
///
/// On Drop, kills the entire process group via `kill_process_by_pid` (Unix:
/// `killpg`, preventing orphaned grandchildren from pipelines). We bypass
/// `Child::start_kill` because that only kills the immediate child.
struct SubprocessGuard {
    /// `None` after clean exit (disarmed) or after explicit reap on timeout.
    child: Option<tokio::process::Child>,
}

impl Drop for SubprocessGuard {
    fn drop(&mut self) {
        if let Some(child) = self.child.take() {
            if let Some(pid) = child.id() {
                // Delegates to the existing platform helper:
                //   Unix:    killpg(pid, SIGKILL) — whole process group
                //   Windows: TerminateProcess on the immediate child
                // Both are best-effort and log on failure; Drop must not panic.
                kill_process_by_pid(Some(pid));
            }
            // We can't `child.wait().await` here (Drop is sync). The
            // immediate-child zombie may persist until the parent process
            // exits — Tokio does NOT auto-reap children dropped without an
            // explicit `wait()`. This is acceptable because:
            //   (a) cancellation is rare (only fires on `scheduler.stop()`),
            //   (b) the OS reaps the zombie when this process exits,
            //   (c) the timeout path explicitly reaps in `execute_command`.
            drop(child);
        }
    }
}

// ============================================================================
// Platform-specific helpers
// ============================================================================

/// Returns the user's login shell from `$SHELL`, falling back to `/bin/sh`.
/// Returns (shell_path, is_login): is_login is false for the fallback.
#[cfg(unix)]
fn shell_path() -> (String, bool) {
    match std::env::var("SHELL") {
        Ok(shell) => (shell, true),
        Err(_) => ("/bin/sh".to_string(), false),
    }
}

#[cfg(windows)]
fn shell_path() -> (&'static str, bool) {
    ("cmd", false)
}

/// Adds the shell flag argument.
/// Unix: `-l -c` for login shells, `-c` for fallback `/bin/sh`.
/// Windows: `/C`.
#[cfg(unix)]
fn shell_arg(cmd: &mut std::process::Command, command: &str, is_login: bool) {
    if is_login {
        cmd.arg("-l");
    }
    cmd.arg("-c").arg(command);
}

#[cfg(windows)]
fn shell_arg(cmd: &mut std::process::Command, command: &str, _is_login: bool) {
    cmd.arg("/C").arg(command);
}

/// Set process group isolation (Unix only — prevents orphaned child processes).
#[cfg(unix)]
fn set_process_group(cmd: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
}

#[cfg(windows)]
fn set_process_group(_cmd: &mut std::process::Command) {
    // On Windows, child processes are naturally terminated when the parent dies
    // via Job Object inheritance. No explicit action needed for our use case.
}

/// PATH element delimiter — `:` on Unix, `;` on Windows.
#[cfg(unix)]
fn path_delimiter() -> &'static str {
    ":"
}

#[cfg(windows)]
fn path_delimiter() -> &'static str {
    ";"
}

/// Kill a process by PID. On Unix, kills the entire process group to prevent orphans.
#[cfg(unix)]
fn kill_process_by_pid(pid: Option<u32>) {
    if let Some(pid) = pid {
        // PID of child is also the PGID since we used process_group(0)
        unsafe {
            if libc::killpg(pid as i32, libc::SIGKILL) != 0 {
                tracing::warn!(
                    "Failed to kill process group {}: {}",
                    pid,
                    std::io::Error::last_os_error()
                );
            }
        }
    }
}

#[cfg(windows)]
fn kill_process_by_pid(pid: Option<u32>) {
    if let Some(pid) = pid {
        // TerminateProcess expects a HANDLE, not a PID. We must OpenProcess
        // first, terminate, then CloseHandle. The previous code cast the PID
        // directly to a HANDLE, which is always invalid — the call silently
        // failed and timed-out subprocesses kept running.
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::System::Threading::{
            OpenProcess, TerminateProcess, PROCESS_TERMINATE,
        };

        unsafe {
            let handle: HANDLE = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if handle.is_null() {
                tracing::warn!(
                    "OpenProcess failed for pid {}: {}",
                    pid,
                    std::io::Error::last_os_error()
                );
                return;
            }
            let terminated = TerminateProcess(handle, 1) != 0;
            if !terminated {
                tracing::warn!(
                    "Failed to terminate process {}: {}",
                    pid,
                    std::io::Error::last_os_error()
                );
            }
            CloseHandle(handle);
        }
    }
}

/// A supported truncation stage of a `| head/tail` pipeline.
#[derive(Debug, Clone, Copy, PartialEq)]
enum TruncationOp {
    Head,
    Tail,
}

/// Truncation stages of a split pipeline: `(op, line_count)` per stage.
type TruncationStages = Vec<(TruncationOp, usize)>;

/// Split a command line into its base command plus an in-process-able
/// truncation pipeline. Models routinely decorate queries as
/// `neomind device list 2>&1 | head -100`; handling the `head/tail/cat`
/// stages in-process keeps the dispatch on the pure-data path (no subprocess
/// auth dependency). Returns `None` when any pipeline stage is unsupported
/// (grep/sort/awk/…) — the caller falls back to the real shell.
///
/// Returns `(base_command, merge_stderr, stages)`; `merge_stderr` is set when
/// the base ends with `2>&1` (stderr is folded into stdout, matching what
/// the shell would have produced).
fn split_truncation_pipeline(trimmed: &str) -> Option<(String, bool, TruncationStages)> {
    let (base_raw, pipe_part) = match trimmed.split_once('|') {
        Some((b, p)) => (b.trim(), Some(p)),
        None => (trimmed, None),
    };
    let (base, merge_stderr) = match base_raw.strip_suffix("2>&1") {
        Some(b) => (b.trim(), true),
        None => (base_raw, false),
    };
    if base.is_empty() {
        return None;
    }
    let mut stages: Vec<(TruncationOp, usize)> = Vec::new();
    if let Some(pipes) = pipe_part {
        for stage in pipes.split('|') {
            let toks: Vec<&str> = stage.split_whitespace().collect();
            let parsed = match toks.as_slice() {
                ["cat"] => None,
                ["head", rest @ ..] | ["tail", rest @ ..] => {
                    let op = if toks[0] == "head" {
                        TruncationOp::Head
                    } else {
                        TruncationOp::Tail
                    };
                    // `head -100` and `head -n 100` are the two forms in use.
                    let n = match rest {
                        [n] => n.strip_prefix('-').unwrap_or(n),
                        ["-n", n] => *n,
                        _ => return None,
                    };
                    Some((op, n.parse::<usize>().ok()?))
                }
                _ => return None,
            };
            if let Some(stage) = parsed {
                stages.push(stage);
            }
        }
    }
    Some((base.to_string(), merge_stderr, stages))
}

/// Fold stderr into stdout (`2>&1`) and apply `head/tail` line truncation,
/// in pipeline order, to an in-process dispatch result.
fn apply_truncation_pipeline(
    mut output: CommandOutput,
    merge_stderr: bool,
    stages: &[(TruncationOp, usize)],
) -> CommandOutput {
    if merge_stderr {
        if !output.stderr.is_empty() {
            if !output.stdout.is_empty() {
                output.stdout.push('\n');
            }
            output.stdout.push_str(&output.stderr);
        }
        output.stderr = String::new();
    }
    for (op, n) in stages {
        let lines: Vec<&str> = if output.stdout.is_empty() {
            Vec::new()
        } else {
            output.stdout.lines().collect()
        };
        let kept: Vec<&str> = match op {
            TruncationOp::Head => lines.into_iter().take(*n).collect(),
            TruncationOp::Tail => {
                let start = lines.len().saturating_sub(*n);
                lines[start..].to_vec()
            }
        };
        output.stdout = kept.join("\n");
    }
    output
}

/// Classify a shell command line for the mutating write gate.
///
/// - Platform CLI (`neomind …`): capability from **parsed** command type
///   (only proven query variants are read-only).
/// - Compound / non-CLI / unparseable: [`ToolCapability::mutating_unknown_sensitive`]
///   — absence of a write keyword is **not** proof of read-only.
pub(crate) fn classify_shell_capability(
    cmd: &str,
) -> crate::agent::task_continuity::ToolCapability {
    classify_shell_capability_with_args(cmd, &serde_json::json!({}))
}

/// Classify a shell tool call using both `command` and structured `args`
/// (e.g. `params`). Strips leaked host-meta tokens (`_new_operation`,
/// `--prepared-operation-id`) before clap parsing so a read-only query does
/// not degrade into `mutating_unknown_sensitive`, and reflects structured
/// `params` (e.g. `extension config <id>` + `params` → `Mutating`).
pub(crate) fn classify_shell_capability_with_args(
    cmd: &str,
    args: &Value,
) -> crate::agent::task_continuity::ToolCapability {
    use crate::agent::task_continuity::ToolCapability;
    use neomind_cli_ops::dispatch::CliSideEffect;

    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return ToolCapability::mutating_unknown_sensitive();
    }

    // Compound sequencing: unknown part ⇒ unknown; else any mutate ⇒ mutate.
    if let Some(parts) = split_shell_sequence(trimmed) {
        if parts.len() > 1 {
            use crate::agent::task_continuity::RetryPolicy;
            let mut any_mutating = false;
            for part in &parts {
                let cap = classify_shell_capability_with_args(part, args);
                match cap.retry_policy {
                    RetryPolicy::Never => {
                        return ToolCapability::mutating_unknown_sensitive();
                    }
                    RetryPolicy::OnFailure { .. } => any_mutating = true,
                    RetryPolicy::AlwaysSafe => {}
                }
            }
            return if any_mutating {
                ToolCapability::mutating_write()
            } else {
                ToolCapability::read_query()
            };
        }
    }

    // Peel safe truncation pipelines (`… 2>&1 | head -N`) before CLI parse.
    let base = match split_truncation_pipeline(trimmed) {
        Some((b, _, _)) => b,
        None => {
            // Unsupported pipe/redir → unknown side-effects (not keyword RO).
            return ToolCapability::mutating_unknown_sensitive();
        }
    };

    let base_trim = base.trim();
    if !(base_trim == "neomind" || base_trim.starts_with("neomind ")) {
        return ToolCapability::mutating_unknown_sensitive();
    }

    let argv = match tokenize_neomind_command(base_trim) {
        Ok(a) => a,
        Err(_) => return ToolCapability::mutating_unknown_sensitive(),
    };
    // Strip leaked host-meta tokens (`_new_operation=true`, `--_new_operation`,
    // `--prepared-operation-id`) before clap classification so a read-only
    // query is never downgraded to `mutating_unknown_sensitive`.
    let mut argv = strip_host_meta_from_argv(argv);

    // Reflect structured shell `params` onto the command's primary JSON flag
    // for classification (e.g. `extension config <id>` with `params` is a write,
    // while bare `extension config <id>` is read-only).
    if let Some(params_val) = args.get("params").filter(|v| v.is_object() || v.is_array()) {
        if let Some(spec) = neomind_cli_ops::json_arg::primary_json_spec(&argv) {
            let file_flag = format!("{}-file", spec.flag);
            if !argv
                .iter()
                .any(|a| a == spec.flag || a.as_str() == file_flag)
            {
                argv.push(spec.flag.to_string());
                argv.push(params_val.to_string());
            }
        }
    }

    match neomind_cli_ops::dispatch::classify_argv(&argv) {
        Ok(CliSideEffect::ProvenReadOnly) => ToolCapability::read_query(),
        Ok(CliSideEffect::Mutating) => ToolCapability::mutating_write(),
        Err(_) => ToolCapability::mutating_unknown_sensitive(),
    }
}

/// Split `&&` / `;` sequences (first separator wins), ignoring empties.
fn split_shell_sequence(trimmed: &str) -> Option<Vec<String>> {
    let has_sep = trimmed.contains("&&") || trimmed.contains(';');
    if !has_sep {
        return None;
    }
    let sep = if let Some(amp) = trimmed.find("&&") {
        if let Some(semi) = trimmed.find(';') {
            if semi < amp {
                ";"
            } else {
                "&&"
            }
        } else {
            "&&"
        }
    } else {
        ";"
    };
    let parts: Vec<String> = trimmed
        .split(sep)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    if parts.len() > 1 {
        Some(parts)
    } else {
        None
    }
}

/// Policy pin: after a successful argv tokenize, `NotInProcess` must spawn
/// that argv — never `/bin/sh -c` of the raw line (quoted `$()` expands).
pub(crate) fn not_in_process_uses_parsed_argv() -> bool {
    true
}

/// NM-240/247: for `extension invoke`, claim the **classify-time** PreparedInvoke
/// (never re-prepare) exclusively from the host-managed [`ExtensionInvokeHostContext`].
/// `strip_host_meta_from_argv` already removes any model-supplied
/// `--prepared-operation-id` tokens before this point, so argv never carries a
/// ticket. Missing/stale → "not executed; resubmit business call".
pub(crate) fn bind_extension_invoke_prepared_token(
    argv: Vec<String>,
) -> std::result::Result<Vec<String>, String> {
    use clap::Parser;
    use neomind_cli_ops::dispatch::commands::{Args, Command, ExtensionCommand};

    let host = neomind_cli_ops::dispatch::peek_extension_invoke_host_context();
    let host_op = host.as_ref().and_then(|h| h.prepared_operation_id.clone());
    let host_params = host.and_then(|h| h.structured_params);

    let Some(op_id) = host_op else {
        let parsed = match Args::try_parse_from(argv.iter()) {
            Ok(a) => a,
            Err(_) => return Ok(argv),
        };
        return match parsed.command {
            Command::Extension {
                extension_cmd:
                    ExtensionCommand::Invoke {
                        extension_id,
                        command,
                        ..
                    },
            } => {
                tracing::warn!(
                    extension_id = %extension_id,
                    command = %command,
                    stage = "shell_bind",
                    "PreparedInvoke missing at Shell bind — host context had no ticket \
                     (likely stripped at execute canonicalize without restore)"
                );
                Err(format!(
                    "PreparedInvoke missing for '{extension_id}/{command}' — not executed; \
                     resubmit the business call (command + params). Do not invent \
                     prepared-operation tickets."
                ))
            }
            _ => Ok(argv),
        };
    };
    tracing::debug!(
        prepared_operation_id = %op_id,
        stage = "shell_bind",
        "claiming PreparedInvoke for Shell → HTTP consume"
    );

    // Ownership check: prefer host structured params, else argv --params.
    let ownership = (|| {
        let parsed = Args::try_parse_from(argv.iter()).ok()?;
        let Command::Extension {
            extension_cmd:
                ExtensionCommand::Invoke {
                    extension_id,
                    command,
                    params,
                    params_file,
                    ..
                },
        } = parsed.command
        else {
            return None;
        };
        let raw = if let Some(v) = host_params.clone() {
            if v.is_null() {
                serde_json::json!({})
            } else {
                v
            }
        } else if let Some(path) = params_file.as_ref() {
            let text = std::fs::read_to_string(path).ok()?;
            serde_json::from_str(text.trim()).ok()?
        } else {
            match params.as_deref() {
                None => serde_json::json!({}),
                Some(s) => serde_json::from_str(s).ok()?,
            }
        };
        Some((extension_id, command, raw))
    })();

    match ownership {
        Some((ext_id, command, raw)) => {
            neomind_core::extension::claim_prepared_for_dispatch(&op_id, &ext_id, &command, &raw)
                .map_err(|e| {
                format!(
                    "PreparedInvoke stale or unbound ('{op_id}'): {e} — not executed; \
                         resubmit the business call (command + params). Do not invent \
                         or reuse prepared-operation tickets."
                )
            })?;
        }
        None => {
            neomind_core::extension::claim_prepared_by_id(&op_id).map_err(|e| {
                format!(
                    "PreparedInvoke stale or unbound ('{op_id}'): {e} — not executed; \
                     resubmit the business call (command + params). Do not invent \
                     or reuse prepared-operation tickets."
                )
            })?;
        }
    }
    Ok(argv)
}

/// Reject when shell `params` conflicts with the command's primary JSON flag
/// (`--params`/`--set`/`--components`/`--ids`/`--body`/`--input`/`--config`),
/// or when `params` has the wrong JSON container kind (object vs array).
pub fn validate_extension_params_sources(args: &Value) -> std::result::Result<(), String> {
    let Some(params_val) = args.get("params") else {
        return Ok(());
    };
    if params_val.is_null() {
        return Ok(());
    }
    if !params_val.is_object() && !params_val.is_array() {
        return Err(
            "shell params must be a JSON object or array (primary JSON payload for --params/--set/--components/--ids/--body/--input/--config)"
                .into(),
        );
    }
    let cmd = args
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if cmd.is_empty() {
        return Ok(());
    }
    let argv = tokenize_neomind_command(cmd).map_err(|e| e.to_string())?;
    let argv = strip_host_meta_from_argv(argv);
    if let Some(contract) = neomind_cli_ops::json_arg::find_domain_command_contract_for_argv(&argv)
    {
        contract.validate_primary_json(params_val)?;
    }
    let Some(spec) = neomind_cli_ops::json_arg::primary_json_spec(&argv) else {
        // Structured params with no known primary flag — ignore (legacy callers).
        return Ok(());
    };
    match spec.kind {
        neomind_cli_ops::json_arg::ExpectedJsonKind::Object if !params_val.is_object() => {
            return Err(format!(
                "shell params for `{}` must be a JSON object",
                spec.flag
            ));
        }
        neomind_cli_ops::json_arg::ExpectedJsonKind::Array if !params_val.is_array() => {
            return Err(format!(
                "shell params for `{}` must be a JSON array",
                spec.flag
            ));
        }
        _ => {}
    }
    let flag = spec.flag;
    let file_flag = format!("{flag}-file");
    if argv.iter().any(|a| a == flag || a.as_str() == file_flag) {
        return Err(format!(
            "conflict: pass JSON via shell `params` OR `{flag}` in command — not both"
        ));
    }
    Ok(())
}

fn extension_invoke_host_context_from_args(
    args: &Value,
) -> Option<neomind_cli_ops::dispatch::ExtensionInvokeHostContext> {
    let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
    let ticket = args
        .get(crate::agent::task_continuity::HOST_PREPARED_OPERATION_ARG)
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let structured = args.get("params").and_then(|v| {
        if v.is_object() || v.is_array() {
            Some(v.clone())
        } else if v.is_null() {
            Some(serde_json::json!({}))
        } else {
            None
        }
    });
    // Prepared ticket only applies to extension invoke; structured JSON applies
    // to any primary-JSON-flag command.
    let is_invoke = cmd.contains("extension invoke") || cmd.contains("extension  invoke");
    if ticket.is_none() && structured.is_none() {
        return None;
    }
    Some(neomind_cli_ops::dispatch::ExtensionInvokeHostContext {
        // Drop ticket for non-invoke — never leak into unrelated domains.
        prepared_operation_id: if is_invoke { ticket } else { None },
        structured_params: structured,
    })
}

/// Strip Chat lifecycle / PreparedInvoke tokens from argv before clap.
///
/// Models sometimes append `_new_operation=true` or `--_new_operation` inside
/// the `command` string. Those are tool-schema fields, never CLI flags.
pub(crate) fn strip_host_meta_from_argv(argv: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(argv.len());
    let mut i = 0;
    while i < argv.len() {
        let tok = argv[i].as_str();
        if is_host_meta_token(tok) {
            i += 1;
            continue;
        }
        if tok == "--prepared-operation-id" {
            i += 1;
            if i < argv.len() {
                i += 1;
            }
            continue;
        }
        if let Some(rest) = tok.strip_prefix("--prepared-operation-id=") {
            let _ = rest;
            i += 1;
            continue;
        }
        out.push(argv[i].clone());
        i += 1;
    }
    out
}

fn is_host_meta_token(tok: &str) -> bool {
    matches!(
        tok,
        "_new_operation"
            | "_new_operation=true"
            | "_new_operation=false"
            | "--_new_operation"
            | "--_new_operation=true"
            | "--_new_operation=false"
            | crate::agent::task_continuity::HOST_PREPARED_OPERATION_ARG
            | crate::agent::task_continuity::PREPARED_OPERATION_ARG
    ) || tok.starts_with("_new_operation=")
        || tok.starts_with("--_new_operation=")
        || tok.starts_with(&format!(
            "{}=",
            crate::agent::task_continuity::HOST_PREPARED_OPERATION_ARG
        ))
        || tok.starts_with(&format!(
            "{}=",
            crate::agent::task_continuity::PREPARED_OPERATION_ARG
        ))
}

/// Inject host structured JSON as the command's primary JSON flag argv pair.
fn inject_structured_json_argv(argv: Vec<String>) -> std::result::Result<Vec<String>, String> {
    let host = neomind_cli_ops::dispatch::peek_extension_invoke_host_context();
    let Some(params) = host.and_then(|h| h.structured_params) else {
        return Ok(argv);
    };
    let Some(flag) = neomind_cli_ops::json_arg::primary_json_flag(&argv) else {
        return Ok(argv);
    };
    let file_flag = format!("{flag}-file");
    if argv.iter().any(|a| a == flag || a.as_str() == file_flag) {
        // validate_extension_params_sources should have caught this; keep
        // host params preferred by dropping the CLI flag pair.
        let mut cleaned = Vec::with_capacity(argv.len());
        let mut i = 0;
        while i < argv.len() {
            if argv[i] == flag || argv[i] == file_flag {
                i += 2;
                continue;
            }
            cleaned.push(argv[i].clone());
            i += 1;
        }
        let mut out = cleaned;
        out.push(flag.to_string());
        out.push(params.to_string());
        return Ok(out);
    }
    let mut out = argv;
    out.push(flag.to_string());
    out.push(params.to_string());
    Ok(out)
}

/// Annotate localhost API transport failures so PreferKnown / recovery can
/// distinguish transport from extension business errors (NM-247 / 10:50 weather).
///
/// Kind comes from [`neomind_cli_ops::ApiClient`] prefixes (`http_timeout:` /
/// `http_connect:` / `http_transport:`) when available:
/// - **connect refuse** → not submitted (body not reached)
/// - **timeout / ambiguous send** → result unknown (request may have been
///   accepted; do not claim "not executed" or auto-resend)
fn classify_extension_transport_error(msg: &str) -> String {
    let lower = msg.to_lowercase();
    let local = lower.contains("9375")
        || lower.contains("localhost")
        || lower.contains("127.0.0.1")
        || lower.contains("/api/extensions/")
        || lower.starts_with("http_timeout:")
        || lower.starts_with("http_connect:")
        || lower.starts_with("http_transport:");
    if !local {
        return msg.to_string();
    }

    let is_timeout = lower.starts_with("http_timeout:")
        || lower.contains("http_timeout:")
        || ((lower.contains("timed out") || lower.contains("timeout"))
            && !lower.contains("http_connect:"));
    let is_connect = lower.starts_with("http_connect:")
        || lower.contains("http_connect:")
        || lower.contains("connection refused")
        || lower.contains("connect error")
        || lower.contains("failed to connect")
        || lower.contains("dns error");
    let looks_transport = is_timeout
        || is_connect
        || lower.starts_with("http_transport:")
        || lower.contains("error sending request");

    if !looks_transport {
        return msg.to_string();
    }

    if is_connect && !is_timeout {
        format!(
            "transport error (localhost API): {msg}. \
             Stage=transport — connection failed before submit; extension \
             business body was not reached (not submitted). PreferKnown of \
             this failure is not a new execution; retry the business call \
             after the API is reachable."
        )
    } else {
        // Timeout or ambiguous send: request may already have hit the server.
        format!(
            "transport error (localhost API): {msg}. \
             Stage=transport — result unknown (request may already have been \
             submitted; do not assume 'not executed'). PreferKnown of a prior \
             failure is not a new execution. Before `_new_operation`, check \
             ledger / inquire status / re-auth; do not auto-resend on timeout alone."
        )
    }
}

/// Tokenize a pure `neomind …` line for clap / capability classification.
///
/// Rejects shell constructs (`|<>$\``) outside quotes so compound / piped
/// lines are not mis-parsed as CLI argv.
pub(crate) fn tokenize_neomind_command(input: &str) -> std::result::Result<Vec<String>, String> {
    // Shell-construct guard: a pipe / redirection / command-substitution char
    // OUTSIDE quotes means this is a real shell command line, not a pure
    // `neomind` invocation — bail so the caller routes it to the subprocess
    // path. (Previously such chars were silently swallowed as ordinary
    // arguments and clap rejected the result: `neomind x | grep y` died as
    // "unexpected argument '|'".)
    {
        let mut in_single = false;
        let mut in_double = false;
        let mut prev = '\0';
        for c in input.chars() {
            match c {
                '\'' if !in_double => in_single = !in_single,
                '"' if !in_single => in_double = !in_double,
                '|' | '<' | '>' | '`' if !in_single && !in_double => {
                    return Err("shell construct outside quotes".to_string())
                }
                '$' if !in_single && !in_double && prev != '\\' => {
                    return Err("shell construct outside quotes".to_string())
                }
                _ => {}
            }
            prev = c;
        }
    }

    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' if !in_single => {
                // Backslash escape: take the next char literally. (Inside
                // single quotes, backslash has no special meaning.)
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }

    if in_single || in_double {
        return Err("unbalanced quotes".to_string());
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Ok(tokens)
}

/// Build the stderr text for a clap parse failure.
///
/// clap's own `Display` already prefixes `error: ...`, so the wrapper adds
/// its own prefix only when missing — the 2026-11-08 eval batch archived
/// doubled `error: error: unexpected argument ...` stderr from the
/// unconditional prefix. For the `rule create` fast-path/JSON flag conflict,
/// append an either-or hint (the clap contract itself is unchanged).
fn parse_error_stderr(parse_error: &str) -> String {
    let mut stderr = if parse_error.starts_with("error:") {
        parse_error.to_string()
    } else {
        format!("error: {parse_error}")
    };
    if parse_error.contains("cannot be used with")
        && parse_error.contains("--name")
        && parse_error.contains("--body")
    {
        stderr.push_str(
            "\nHint: choose ONE form — fast path: --name <N> --trigger-device <D> \
             --metric <M> --operator <OP> --threshold <V>; or full JSON rule: \
             --body '<rule JSON>'. `neomind rule create --help` shows both.",
        );
    }
    stderr
}

/// Conservatively recognize the split-path parse failure without guessing at
/// arbitrary extra words. Tokenization preserves quoted/escaped spaces inside
/// one argument, so only adjacent, whitespace-free path fragments qualify.
fn should_hint_split_path(parse_error: &str, argv: &[String]) -> bool {
    let Some((_, rest)) = parse_error.split_once("unexpected argument '") else {
        return false;
    };
    let Some((unexpected, _)) = rest.split_once('\'') else {
        return false;
    };
    let rooted = |s: &str| {
        s.starts_with('/')
            || s.starts_with("./")
            || s.starts_with("../")
            || s.starts_with("~/")
            || s.starts_with("\\\\")
            || (s.as_bytes().get(1) == Some(&b':')
                && matches!(s.as_bytes().get(2), Some(b'/' | b'\\')))
    };
    // A bare extra word is ambiguous; a second independent absolute path is
    // not evidence that it belongs to the preceding argument either.
    if unexpected.starts_with('-')
        || unexpected.chars().any(char::is_whitespace)
        || !unexpected.contains(['/', '\\'])
        || rooted(unexpected)
    {
        return false;
    }
    argv.windows(2).any(|pair| {
        pair[1] == unexpected && rooted(&pair[0]) && !pair[0].chars().any(char::is_whitespace)
    })
}

/// Truncate stdout + stderr to fit within max_total chars, with truncation notices.
fn truncate_output(stdout: &str, stderr: &str, max_total: usize) -> (String, String) {
    let stdout_len = stdout.len();
    let stderr_len = stderr.len();

    if stdout_len + stderr_len <= max_total {
        return (stdout.to_string(), stderr.to_string());
    }

    // Reserve space for truncation notices
    const NOTICE_LEN: usize = 60;
    let usable = max_total.saturating_sub(NOTICE_LEN * 2);

    let total = stdout_len + stderr_len;
    let stdout_budget = if total > 0 {
        usable
            .checked_mul(stdout_len)
            .map(|p| (p / total).min(stdout_len))
            .unwrap_or(stdout_len)
    } else {
        usable / 2
    };
    let stderr_budget = usable.saturating_sub(stdout_budget).min(stderr_len);

    let truncated_stdout = if stdout_len > stdout_budget {
        let safe_end = find_safe_truncation_point(stdout, stdout_budget);
        format!(
            "{}\n... [truncated, {} chars omitted]",
            &stdout[..safe_end],
            stdout_len - safe_end
        )
    } else {
        stdout.to_string()
    };

    let truncated_stderr = if stderr_len > stderr_budget {
        let safe_end = find_safe_truncation_point(stderr, stderr_budget);
        format!(
            "{}\n... [truncated, {} chars omitted]",
            &stderr[..safe_end],
            stderr_len - safe_end
        )
    } else {
        stderr.to_string()
    };

    (truncated_stdout, truncated_stderr)
}

/// Find a safe byte position to truncate at (don't split multi-byte UTF-8 chars).
fn find_safe_truncation_point(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() {
        return s.len();
    }
    let mut pos = max_bytes;
    while pos > 0 && !s.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

// ============================================================================
// Error Recovery Hints
// ============================================================================

/// Return `true` when `stdout` is a JSON `CliResponse` / error payload that
/// already carries a non-empty `suggestion` or `hint` (at top level, under
/// `error`, or under `data`). Since Phase 2 keeps `stdout` pure JSON, we parse
/// it directly instead of substring-matching raw text.
fn stdout_has_server_guidance(stdout: &str) -> bool {
    fn non_empty_str(v: &serde_json::Value, key: &str) -> bool {
        v.get(key)
            .and_then(|s| s.as_str())
            .is_some_and(|s| !s.trim().is_empty())
    }
    fn obj_has_guidance(v: &serde_json::Value) -> bool {
        non_empty_str(v, "suggestion") || non_empty_str(v, "hint")
    }
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return false;
    }
    let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return false;
    };
    obj_has_guidance(&val)
        || val.get("error").is_some_and(obj_has_guidance)
        || val.get("data").is_some_and(obj_has_guidance)
}

/// Return `true` when `stderr` already carries a server-originated `Hint:`
/// line extracted from `ErrorResponse.hint` by `ApiClient`.
fn stderr_has_server_guidance(stderr: &str) -> bool {
    stderr.lines().any(|line| {
        line.trim_start()
            .strip_prefix("Hint:")
            .is_some_and(|rest| !rest.trim().is_empty())
    })
}

/// True for targeted parse-error hints that add value alongside clap's
/// in-process `usage_hint` (as opposed to broad domain action summaries).
fn is_targeted_parse_hint(hint: &str) -> bool {
    hint.starts_with("ID is a positional argument")
        || hint.starts_with("Device-defined command parameters")
}

impl ShellTool {
    /// Generate a recovery hint when a neomind CLI command fails.
    fn recovery_hint(command: &str, stdout: &str, stderr: &str) -> Option<String> {
        let cmd = command.trim();
        let argv = tokenize_neomind_command(cmd)
            .unwrap_or_else(|_| cmd.split_whitespace().map(str::to_string).collect());
        let argv = strip_host_meta_from_argv(argv);
        if argv.first().map(String::as_str) != Some("neomind") {
            return None;
        }

        // If the server / CLI already returned structured guidance in JSON
        // `stdout` (`suggestion` / `hint`) or extracted `Hint:` in `stderr`,
        // do not overwrite or stack a generic hint that could contradict it.
        if stdout_has_server_guidance(stdout) || stderr_has_server_guidance(stderr) {
            return None;
        }

        let domain = argv.get(1).map(String::as_str).unwrap_or("");
        let action = argv.get(2).map(String::as_str).unwrap_or("");
        let combined = format!("{} {}", stdout, stderr).to_lowercase();

        let is_not_found = combined.contains("not found")
            || combined.contains("404")
            || combined.contains("does not exist")
            || combined.contains("no such");
        let is_validation = combined.contains("validation")
            || combined.contains("invalid")
            || combined.contains("missing")
            || combined.contains("required")
            || combined.contains("400")
            || combined.contains("422");
        let is_unexpected_arg = combined.contains("unexpected argument")
            || combined.contains("unexpected flag")
            || combined.contains("unrecognized argument")
            || combined.contains("unused arguments")
            || combined.contains("error: found argument");
        let has_id_flag = argv
            .iter()
            .skip(1)
            .any(|tok| tok == "--id" || tok.starts_with("--id="));
        let missing_positional_id = combined.contains("required arguments were not provided")
            && (combined.contains("<id>")
                || combined.contains("<device_id>")
                || combined.contains("<dashboard_id>")
                || combined.contains("<rule_id>")
                || combined.contains("<agent_id>")
                || combined.contains("<extension_id>"));

        // Targeted syntax hints only — never blindly tell the model "ID is
        // positional" on unrelated flag errors, and avoid fighting with
        // clap's in-process `usage_hint` on exit_code 2.
        if is_unexpected_arg || missing_positional_id {
            if has_id_flag || missing_positional_id {
                return Some("ID is a positional argument, not a flag. Use: neomind <domain> <action> <ID> [options]. Example: neomind device get abc123 (not --id abc123).".to_string());
            }
            if domain == "device" && action == "control" {
                return Some("Device-defined command parameters are not standalone CLI flags. Use neomind device control <ID> <COMMAND> --param 'key=value' (repeat for each parameter), or --params '<JSON>'. For a text parameter, use --param 'text=your content', not --text. Use device get <ID> to check actual parameter names and types; do not invent keys. Run neomind device control --help for CLI options.".to_string());
            }
            return None;
        }

        match domain {
            "device" => {
                if action == "control" && is_not_found {
                    Some("Device or command not found. Run 'neomind device list' to verify the device ID and 'neomind device get <ID>' to inspect supported commands, then call 'neomind device control <ID> <COMMAND>' with shell `params` (COMMAND is positional, not a flag).".to_string())
                } else if is_not_found {
                    Some("Run 'neomind device list' to see available devices, then retry with a valid ID.".to_string())
                } else if action == "create" && is_validation {
                    Some("Required flags: --name, --device-type, --adapter-type (mqtt|webhook). Use 'neomind device types list' to see built-in types.".to_string())
                } else if action == "control" && is_validation {
                    // Server already returns schema + params_example + "NOT sent".
                    // Do NOT overwrite that with the generic device subcommand list.
                    if combined.contains("hint:")
                        || combined.contains("params_example")
                        || combined.contains("not sent")
                        || combined.contains("protocol envelope")
                    {
                        None
                    } else {
                        Some(
                            "Pass the command's own param object from `device get` via shell `params` (or --params; see shell_example / params_example). \
                             Do not wrap as protocol {\"cmd\":...,\"params\":{...}}; the host builds that envelope. \
                             Validation failure means the command was NOT sent."
                                .to_string(),
                        )
                    }
                } else if (action == "history" || action == "latest") && combined.contains("metric")
                {
                    Some("Don't guess metric names. 'neomind device list' shows devices only — it carries no field names. Use 'neomind device get <ID>' for one device's actual metric fields, or 'neomind device types list' for a type's metric_fields.".to_string())
                } else {
                    Some("Available actions: list, get, create, update, delete, latest, history, control, write-metric, webhook-url, types, drafts. ID is positional: neomind device <action> <ID> [flags].".to_string())
                }
            }
            "dashboard" => {
                if is_not_found {
                    Some("Run 'neomind dashboard list' to see available dashboards.".to_string())
                } else if action == "create" && is_validation {
                    Some("Required field: --name. Example: neomind dashboard create --name \"My Dashboard\"".to_string())
                } else if action == "update" {
                    Some("update --components full-replaces the widget array and requires --replace-all. For ONE widget use shell with command=\"neomind dashboard update-component <ID> --component-id <CID>\" and params={...} (deep-merge patch); to add widgets use add-components with params=[...]; to remove use remove-components with params=[\"<CID>\"].".to_string())
                } else if is_validation {
                    if let Some(contract) =
                        neomind_cli_ops::json_arg::find_domain_command_contract(domain, action)
                    {
                        Some(contract.format_hint())
                    } else {
                        Some("Available actions: list, get, create, update, add-components, update-component, remove-components, delete, share.".to_string())
                    }
                } else {
                    Some("Available actions: list, get, create, update, add-components, update-component, remove-components, delete, share.".to_string())
                }
            }
            "rule" => {
                if is_not_found {
                    Some("Run 'neomind rule list' to see available rules.".to_string())
                } else if action == "create"
                    && (is_validation || combined.contains("json") || combined.contains("parse"))
                {
                    Some("Rule JSON format: {\"name\":\"...\",\"condition\":{\"condition_type\":\"comparison\",\"source\":\"device:SENSOR_ID:METRIC\",\"operator\":\"greater_than\",\"threshold\":30},\"actions\":[{\"type\":\"notify\",\"message\":\"Alert\",\"severity\":\"critical\"}]}. BEFORE creating: run `neomind device list` to discover real device IDs and metric_fields per type. NEVER guess device IDs or metric names.".to_string())
                } else if action == "enable" || action == "disable" {
                    Some("Run 'neomind rule list' to find the rule ID, then 'neomind rule <enable|disable> <ID>'.".to_string())
                } else if is_validation {
                    if let Some(contract) =
                        neomind_cli_ops::json_arg::find_domain_command_contract(domain, action)
                    {
                        Some(contract.format_hint())
                    } else {
                        Some("Available actions: list, get, create, update, delete, enable, disable, test, history".to_string())
                    }
                } else {
                    Some("Available actions: list, get, create, update, delete, enable, disable, test, history".to_string())
                }
            }
            "agent" => {
                if is_not_found {
                    Some("Run 'neomind agent list' to see available agents.".to_string())
                } else if action == "create" && is_validation {
                    Some("Required fields: --name, --prompt, --schedule-type (event|interval|cron|manual). Example: neomind agent create --name \"monitor\" --prompt \"Check devices\" --schedule-type event. Never invent device IDs or metric names: run 'neomind device list' for real IDs, then 'neomind device get <ID>' for that device's metric fields. A structured agent with no bound source refuses to run.".to_string())
                } else if action == "control" && is_validation {
                    Some("Status is positional: neomind agent control <ID> <active|paused>. Example: neomind agent control abc123 active".to_string())
                } else if is_validation {
                    if let Some(contract) =
                        neomind_cli_ops::json_arg::find_domain_command_contract(domain, action)
                    {
                        Some(contract.format_hint())
                    } else {
                        Some("Available actions: list, get, create, update, delete, control, invoke, memory, executions, latest-execution, conversation, send-message".to_string())
                    }
                } else {
                    Some("Available actions: list, get, create, update, delete, control, invoke, memory, executions, latest-execution, conversation, send-message".to_string())
                }
            }
            "extension" => {
                if is_not_found {
                    Some("Overview: 'neomind extension list'. Concrete task: 'neomind extension commands --search \"<task>\"' then paste hit shell_example. Known id: 'neomind extension commands <ID>'.".to_string())
                } else if action == "install" && is_validation {
                    Some("Provide the extension zip file path. Use 'neomind extension market-list' to browse marketplace.".to_string())
                } else if action == "config" {
                    Some("Usage: `neomind extension config <ID>` to view, or shell with command=\"neomind extension config <ID>\" and params={\"key\":\"value\"} to update.".to_string())
                } else if action == "commands" || action == "invoke" {
                    Some("Concrete task: neomind extension commands --search \"<task>\" (use shell_example). Overview: extension list — do not search \"extension capabilities\". Reuse known on follow-up.".to_string())
                } else {
                    Some("Available actions: list, get, commands, invoke, status, logs, config, install, uninstall, reload, create, build, market-list, market-install, validate".to_string())
                }
            }
            "transform" => {
                if is_not_found {
                    Some("Run 'neomind transform list' to see available transforms.".to_string())
                } else if action == "create" && is_validation {
                    Some("Required flags: --name, --scope (global|device_type:X|device:ID), --code (JavaScript; the input value is bound to `input`). Example: neomind transform create --name \"celsius\" --code \"return input * 1.8 + 32\" --scope global".to_string())
                } else {
                    Some("Available actions: list, get, create, update, enable, disable, delete, test-code, metrics, data-sources, executions".to_string())
                }
            }
            "widget" => {
                if is_not_found {
                    Some(
                        "Run 'neomind widget list' to see available widgets (built-in + custom)."
                            .to_string(),
                    )
                } else if action == "create" && is_validation {
                    Some("Valid widget types: chart, gauge, stat, table, image, custom. Example: neomind widget create \"My Chart\" --widget-type chart".to_string())
                } else if action == "install" && is_validation {
                    Some("Provide a widget directory (containing manifest.json + bundle.js) or a .zip file. Use DATA-RELATIVE paths (no leading data/): neomind widget install frontend-components/my-widget — absolute ~/Library/Application Support/... paths contain a space and need quotes".to_string())
                } else {
                    Some("Available actions: list, get, create, install, uninstall, market-list, market-install".to_string())
                }
            }
            "message" => {
                if is_not_found {
                    Some("Run 'neomind message list' to see all messages.".to_string())
                } else if action == "send" && is_validation {
                    Some("Required fields: --title, --body, --severity (info|warning|critical|emergency). Example: neomind message send --title \"Alert\" --body \"High temp\" --severity warning".to_string())
                } else if action == "channel-update" {
                    Some("Usage: shell with command=\"neomind message channel-update --name <N>\" and params={\"min_severity\":\"warning\"} (or --config '<JSON>'). To filter by source type: params={\"source_types\":[\"device\"]}. channel-create uses --name flag; channel-delete/channel-test take name as positional arg.".to_string())
                } else {
                    Some("Available actions: list, get, send, read, delete, channel-list, channel-get, channel-types, channel-type-schema, channel-create, channel-update, channel-delete, channel-test.".to_string())
                }
            }
            "llm" => {
                if is_not_found {
                    Some("Run 'neomind llm list' to see configured backends.".to_string())
                } else if action == "create" && is_validation {
                    Some("Required fields: --name, --type (ollama|llamacpp|openai|anthropic), --endpoint, --model. Example: neomind llm create --name local --type ollama --endpoint http://localhost:11434 --model qwen3.5:4b".to_string())
                } else {
                    Some("Available actions: list, get, models, create, update, delete, activate, test. Example: neomind llm create --name local --type ollama --endpoint http://localhost:11434 --model qwen3.5:4b".to_string())
                }
            }
            _ => Some(format!(
                "Run 'neomind {domain} --help' for the exact actions and flags. Quick map — connector: list,get,create,test,enable,disable,subscribe. push: get,create,test,enable,disable,logs. settings: timezone,timezones,retention,cleanup. system: info."
            )),
        }
    }

    /// Hint for "silent success" commands — exit 0 with empty stdout AND stderr.
    ///
    /// GUI launchers (`open`, `xdg-open`, `start`, `explorer`, `see`) and a
    /// few other commands return no output on success. Without a hint the LLM
    /// has no feedback to confirm the action took effect and tends to retry
    /// with cosmetic variants (`open -a Preview`, `open -R`, etc.) hoping for
    /// output that will never come. Each variant produces a different
    /// dedup-signature so the cross-round dedup doesn't catch the loop either.
    ///
    /// Returns `Some(hint)` only when the command's first token is a known
    /// silent-success launcher; `None` otherwise (so genuinely empty-output
    /// commands like `mkdir` keep their plain result).
    fn silent_success_hint(command: &str) -> Option<String> {
        // Find the first non-env-assignment token. Skips `KEY=value` prefixes
        // like `DISPLAY=:0` so the bare-command check lands on the real binary.
        let first = command
            .split_whitespace()
            .find(|t| !t.contains('='))?
            .trim_matches('"');

        const LAUNCHERS: &[&str] = &[
            "open",      // macOS
            "xdg-open",  // Linux
            "gio",       // Linux GNOME (gio open)
            "start",     // Windows (rare via sh -c, but covered)
            "explorer",  // Windows
            "see",       // macOS alternative
            "launchctl", // macOS service loader (load/start substrings)
        ];

        if !LAUNCHERS.contains(&first) {
            return None;
        }

        Some(format!(
            "Command '{}' completed successfully with no output. This is expected for GUI-launching commands — the application was told to open, but you cannot see its window and cannot perceive the result by retrying. Do NOT call '{}' again or try variants (different flags, -a <app>, -R, etc.); they all return the same empty output. Move on to the next step of your task; if you needed to inspect the visual content, ask the user.",
            first, first
        ))
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        // Slim description (canonical): Critical Syntax Rules (hard
        // constraints) + CLI concept. The per-domain subcommand INDEX lives
        // in DOMAIN_INDEX below and is INJECTED into the first shell tool
        // result instead of living here — the 2026-08-18 full eval proved a
        // 2600-char static description re-triggers the ≤3B tool-selection
        // suppression (models grabbed file_write/web_fetch instead of shell:
        // the description-avoidance signature from the 6510-char era), which
        // cost more cases than the index's recall gained. Defensive knowledge
        // (easy-to-miss list, GUI guard) stays out — discoverable via
        // `neomind <domain> <action> --help` or the skill tool.
        static SLIM: &str = r#"Execute shell commands on the host. This is your PRIMARY tool for ALL NeoMind platform operations via the `neomind` CLI (14 domains: device, dashboard, rule, agent, extension, widget, transform, llm, message, connector, push, settings, system, api-key). All commands return JSON by default — do NOT pass --json.

Quick reference (run `neomind <domain> --help` or load the `skill` guide for full syntax):
- Read: `<domain> get <ID>` (one) / `<domain> list` (all). ID is positional, never `--id`.
- Write: `<domain> create/update/delete` take flags (`--name`, `--device-type`, …). Pass `--id <id>` on create ONLY if the user gave a specific ID.
- Control / enable / activate are explicit writes: `device control <ID> <CMD> --param key=value` (repeat for each device-defined parameter; these are NOT standalone `--key` flags), `rule enable <ID>`, `llm activate <ID>`, `connector enable <ID>`, `push enable <ID>`.
- Extensions: overview → `extension list`; concrete task → `extension commands --search "<task>"` (paste `shell_example`); known → reuse invoke; schema → `extension commands <ID>`. Do not invent sanitized chat-tool names as CLI subcommands.
- History & conversation: `agent executions <ID>`, `agent conversation <ID>`, `agent send-message <ID>`.
- Notification channels are a sub-family: `message channel-list` / `channel-create` / `channel-test`, NOT `message list`.

Critical rules:
- NEVER guess metric or subcommand names — discover via `get`/`list`/`--help` first, then use exact names.
- Read before write when facts are missing or stale. Reuse verified target IDs and schemas from this conversation; inspect only missing or stale facts before writing.
- COMPLETE THE FULL FLOW: perform requested actions and verify their results. Example: a request to create, enable and test a connector needs connector create, connector enable, then connector test. Configuration saved is not proof of device feedback.
- On error, read the `suggestion` field in the JSON output for recovery.

Native host tools also available via `/bin/sh -c`: ping, curl, ps, df, grep, docker, …"#;
        SLIM
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "command": {
                    "type": "string",
                    "description": "The shell command to execute. For `neomind` commands, use built-in filter flags (no `| grep/jq`); only native host commands support pipes and redirections. For neomind commands that take JSON (--params/--set/--components/--ids/--body/--input/--config), prefer the structured `params` field instead of embedding quoted JSON in this string. Never put `_new_operation` inside this string — it is a separate tool field."
                },
                "params": {
                    "anyOf": [
                        { "type": "object" },
                        { "type": "array" }
                    ],
                    "description": "Optional structured JSON payload (object or array) for the command's primary JSON flag (--params for device control / extension invoke, --set for dashboard update-component / extension config, --components/--ids array for dashboard add/remove-components, --body for rules, --input for rule test / transform test-code, --config for device/push/message channel). Prefer this over quoting JSON in `command`. Do not also pass the same flag in `command`."
                },
                "timeout": {
                    "type": "number",
                    "description": "Optional per-command timeout in seconds (max 600). Overrides default timeout."
                },
                "description": {
                    "type": "string",
                    "description": "Brief description of what this command does (5-10 words). Used for logging and audit."
                },
                "working_dir": {
                    "type": "string",
                    "description": "Optional working directory for command execution. Must be an existing directory path."
                }
            }),
            vec!["command".to_string()],
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn clone_for_chat_session(&self, session_id: &str) -> Option<super::tool::DynTool> {
        Some(std::sync::Arc::new(self.for_chat_session(session_id)))
    }

    fn reset_session_state(&self) {
        self.index_injected
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    fn execution_timeout(&self, args: &Value) -> Result<Duration> {
        Ok(self.command_timeout(args)? + Duration::from_secs(5))
    }

    async fn execute(&self, args: Value) -> Result<ToolOutput> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("command is required".into()))?;

        if command.trim().is_empty() {
            return Err(ToolError::InvalidArguments(
                "command cannot be empty".into(),
            ));
        }

        // NM-247: structured params adapter — object XOR --params/--params-file.
        if let Err(msg) = validate_extension_params_sources(&args) {
            return Err(ToolError::InvalidArguments(msg));
        }

        let timeout = self.command_timeout(&args)?;

        let working_dir = args.get("working_dir").and_then(|v| v.as_str());
        let description = args.get("description").and_then(|v| v.as_str());

        tracing::info!(
            command = %command,
            description = description.unwrap_or(""),
            "Executing shell command"
        );

        // Host ticket + structured params scoped to this async task only.
        let host_ctx = extension_invoke_host_context_from_args(&args);
        let output = if let Some(ctx) = host_ctx {
            neomind_cli_ops::dispatch::with_extension_invoke_host_context(ctx, async {
                self.execute_command(command, working_dir, timeout).await
            })
            .await?
        } else {
            self.execute_command(command, working_dir, timeout).await?
        };

        // execute_command already truncated subprocess output; in-process
        // `neomind` output is left intact so the streaming slim layer can
        // cache image/base64 payloads as `$cached` refs before any size cap
        // destroys them. (stdout/stderr are moved out; exit_code/timed_out
        // are still read below.)
        let stdout = output.stdout;
        let stderr = output.stderr;

        tracing::info!(
            command = %command,
            exit_code = ?output.exit_code,
            timed_out = output.timed_out,
            stdout_len = stdout.len(),
            stderr_len = stderr.len(),
            "Shell command completed"
        );

        let mut result = serde_json::json!({
            "exit_code": output.exit_code,
            "stdout": stdout,
            "stderr": stderr,
            "command": command,
            "timed_out": output.timed_out
        });
        if let Some(desc) = description {
            result["description"] = serde_json::Value::String(desc.to_string());
        }
        if let Some(dir) = working_dir {
            result["working_dir"] = serde_json::Value::String(dir.to_string());
        }
        if let Some(domain_index) = output.domain_index {
            result["domain_index"] = serde_json::Value::String(domain_index);
        }
        if let Some(usage_hint) = output.usage_hint {
            result["usage_hint"] = serde_json::Value::String(usage_hint);
        }

        // Enrich error responses with recovery hints for neomind CLI commands
        let is_error = output.exit_code.unwrap_or(1) != 0;
        let has_usage_hint = result.get("usage_hint").is_some();
        if is_error {
            if let Some(hint) = Self::recovery_hint(command, &stdout, &stderr) {
                if !has_usage_hint || is_targeted_parse_hint(&hint) {
                    result["suggestion"] = serde_json::Value::String(hint);
                }
            }
        } else if stdout.is_empty() && stderr.is_empty() {
            // Success but zero output — typical of GUI launchers (`open`,
            // `xdg-open`, `start`, `explorer`). Without a hint the LLM tends to
            // retry endlessly because it has no signal the action took effect.
            if let Some(hint) = Self::silent_success_hint(command) {
                result["note"] = serde_json::Value::String(hint);
            }
        }

        Ok(ToolOutput::success(result))
    }
}

/// Detect `echo`/`printf` commands whose payload impersonates a tool
/// result (JSON with `exit_code` + `command`/`stdout` keys). Returns the
/// `neomind` command mentioned inside the payload, if any, for the cure
/// message.
fn fabricated_tool_output(command: &str) -> Option<String> {
    let trimmed = command.trim();
    let is_print = trimmed.starts_with("echo ")
        || trimmed.starts_with("printf ")
        || trimmed.starts_with("echo\t")
        || trimmed.contains("| echo ");
    if !is_print {
        return None;
    }
    // Bare substrings, not quote-delimited: the payload's quotes arrive
    // shell-escaped (`\"exit_code\"`), so a quoted match never fires.
    let looks_like_tool_result = trimmed.contains("exit_code")
        && (trimmed.contains("command") || trimmed.contains("stdout"));
    if !looks_like_tool_result {
        return None;
    }
    // Extract the real command the payload impersonates, for the cure:
    // text after the last `command` key up to the closing brace, stripped
    // of JSON punctuation and shell-escape backslashes.
    let real: String = command
        .rsplit("command")
        .next()
        .unwrap_or("")
        .split('}')
        .next()
        .unwrap_or("")
        .replace(['\\', '"', ':', '{'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (real.len() > 3).then_some(real)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_description_maps_device_parameters_to_param() {
        let description = ShellTool::new(ShellConfig::default())
            .description()
            .to_owned();
        assert!(description.contains("device control <ID> <CMD> --param key=value"));
        assert!(description.contains("NOT standalone `--key` flags"));
    }

    #[test]
    fn control_parameter_flag_error_has_targeted_recovery() {
        let error = "error: unexpected argument '--text' found";
        let hint = ShellTool::recovery_hint(
            "neomind device control eink-test render_text --text 'weather'",
            "",
            error,
        )
        .unwrap();
        assert!(hint.contains("--param 'text=your content'"));
        assert!(hint.contains("--params '<JSON>'"));
        assert!(hint.contains("actual parameter names and types"));
        assert!(!hint.contains("ID is positional"));
    }

    #[test]
    fn control_hint_does_not_shadow_id_or_other_error_contracts() {
        let hint = |cmd, error| ShellTool::recovery_hint(cmd, "", error).unwrap();
        let error = "error: unexpected argument '--id' found";
        assert!(
            hint("neomind device control --id eink-test render_text", error)
                .starts_with("ID is a positional argument")
        );
        assert!(hint(
            "neomind device get --id=eink-test",
            "error: unexpected argument '--id=eink-test' found"
        )
        .starts_with("ID is a positional argument"));
        assert!(hint(
            "neomind device get",
            "error: the following required arguments were not provided:\n  <ID>"
        )
        .starts_with("ID is a positional argument"));
        // Unrelated unexpected flag must NOT blindly claim "ID is positional"
        // (and avoids fighting with clap's in-process `usage_hint`).
        assert!(ShellTool::recovery_hint(
            "neomind rule create --bogus 1",
            "",
            "error: unexpected argument '--bogus' found",
        )
        .is_none());
        // `action == "control" && is_not_found` is no longer shadowed by generic `is_not_found`.
        let ctrl_nf = hint(
            "neomind device control eink-test bad_command",
            "Command not found",
        );
        assert!(
            ctrl_nf.contains("COMMAND is positional"),
            "control not-found branch must be reachable: {ctrl_nf}"
        );
        assert!(!ctrl_nf.contains("--param"));
        // Server schema / NOT-sent feedback must not be replaced by the
        // generic "Available actions" device subcommand list.
        assert!(ShellTool::recovery_hint(
            "neomind device control eink-test render_text --params '{\"cmd\":\"render_text\",\"params\":{\"text\":\"hi\"}}'",
            "",
            "Failed to send command: Command 'render_text' was NOT sent: missing required top-level params: text. Example: {\"text\":\"Hello\"}.\nHint: {\"params_example\":{\"text\":\"Hello\"}}",
        )
        .is_none());
        // When `stdout` is a JSON CliResponse that already carries `suggestion` or `hint`,
        // `recovery_hint` must not overwrite or layer a generic hint on top.
        assert!(ShellTool::recovery_hint(
            "neomind device get missing-1",
            r#"{"success":false,"error":"Device not found","suggestion":"Run 'neomind device list' first."}"#,
            "",
        )
        .is_none());
        assert!(ShellTool::recovery_hint(
            "neomind rule create --body '{}'",
            r#"{"success":false,"error":{"code":"VALIDATION_ERROR","message":"bad","hint":"Use greater_than"}}"#,
            "",
        )
        .is_none());
        // When `stderr` carries a server-extracted `Hint:` line (e.g. 422 from
        // `POST /messages/channels`), `recovery_hint` must not stack generic copy.
        assert!(ShellTool::recovery_hint(
            "neomind message channel-create --name ops --type webhook --config '{}'",
            "",
            "API error (422 Unprocessable Entity): Missing required config field(s) for 'webhook' channel: url\nHint: Required fields for 'webhook': url. Run `neomind message channel-type-schema webhook`.",
        )
        .is_none());
        let control_validation = hint(
            "neomind device control eink-test render_text --params '{}'",
            "Failed to send command: missing required params: text",
        );
        assert!(
            control_validation.contains("params_example")
                || control_validation.contains("NOT sent")
                || control_validation.contains("param object"),
            "{control_validation}"
        );
        assert!(!control_validation.contains("Available actions"));
        assert!(ShellTool::recovery_hint("echo test", "", error).is_none());
    }

    #[test]
    fn parse_error_stderr_carries_exactly_one_error_prefix() {
        // clap's Display already prefixes "error:" — the wrapper must not
        // double it (2026-11-08 batch archived "error: error: ...").
        let s = parse_error_stderr("error: unexpected argument '--x' found");
        assert!(s.starts_with("error: "));
        assert_eq!(s.matches("error:").count(), 1);
        // Defensive: a message without clap's prefix still gets exactly one.
        let s2 = parse_error_stderr("unexpected argument");
        assert!(s2.starts_with("error: unexpected"));
        assert_eq!(s2.matches("error:").count(), 1);
    }

    #[test]
    fn parse_error_stderr_hints_rule_create_name_body_conflict() {
        let s = parse_error_stderr(
            "error: the argument '--name <NAME>' cannot be used with '--body <BODY>'",
        );
        assert!(s.contains("choose ONE form"));
        assert!(s.contains("--trigger-device"));
        // Conflicts NOT involving both --name and --body get no hint.
        let other =
            parse_error_stderr("error: the argument '--force' cannot be used with '--json'");
        assert!(!other.contains("choose ONE form"));
    }

    #[test]
    fn truncation_pipeline_splits_head_decorations() {
        let (base, merge, stages) =
            split_truncation_pipeline("neomind device list 2>&1 | head -100").unwrap();
        assert_eq!(base, "neomind device list");
        assert!(merge);
        assert_eq!(stages, vec![(TruncationOp::Head, 100)]);
    }

    #[test]
    fn truncation_pipeline_supports_n_form_and_composition() {
        let (base, merge, stages) =
            split_truncation_pipeline("neomind rule list | head -n 20 | tail -n 5").unwrap();
        assert_eq!(base, "neomind rule list");
        assert!(!merge);
        assert_eq!(
            stages,
            vec![(TruncationOp::Head, 20), (TruncationOp::Tail, 5)]
        );
    }

    #[test]
    fn truncation_pipeline_rejects_unsupported_stages() {
        assert!(split_truncation_pipeline("neomind device list | grep temp").is_none());
        assert!(split_truncation_pipeline("neomind device list | sort").is_none());
    }

    #[test]
    fn truncation_pipeline_passes_plain_commands_through() {
        let (base, merge, stages) = split_truncation_pipeline("neomind device list").unwrap();
        assert_eq!(base, "neomind device list");
        assert!(!merge);
        assert!(stages.is_empty());
    }

    #[test]
    fn truncation_applied_head_then_tail_in_order() {
        let out = CommandOutput::plain(Some(0), "1\n2\n3\n4\n5".into(), String::new(), false);
        let out = apply_truncation_pipeline(
            out,
            false,
            &[(TruncationOp::Head, 3), (TruncationOp::Tail, 2)],
        );
        assert_eq!(out.stdout, "2\n3");
    }

    fn test_config() -> ShellConfig {
        ShellConfig {
            enabled: true,
            timeout_secs: 10,
            max_output_chars: 5000,
        }
    }

    /// Regression guard: the shell tool's Command Choice must keep the exact
    /// subcommand disambiguation for the domains GLM-5.2 failed on
    /// (extension/message/agent/widget/transform-metrics). A prompt slim that
    /// drops these lines silently reintroduces command-variant failures — the
    /// model then improvises subcommands (extension info↔get, data-sources↔metrics)
    /// because those domains aren't in the always-present reference.
    /// The skeleton description replaced the old dense per-domain Command
    /// Choice block (6510 chars) — that block suppressed tool SELECTION on
    /// ≤3B models (they avoided the huge description and grabbed `skill`
    /// instead; verified A/B on LFM2.5-VL-3B). The load-bearing property is
    /// that the description stays SHORT; per-domain subcommand syntax is
    /// delivered on demand via `--help` injection (see `domain_help`).
    #[test]
    fn skeleton_description_stays_concise() {
        let tool = ShellTool::new(test_config());
        let d = tool.description();
        assert!(
            d.len() < 2600,
            "shell description grew to {} chars — dense descriptions suppress \
             tool selection on <=3B models; move detail to on-demand injection",
            d.len()
        );
        // Skeleton essentials
        let dl = d.to_lowercase();
        for needle in [
            "primary tool",      // positions shell as the default
            "quick reference",   // points at --help / skill for detail
            "read before write", // sequence directive
            "complete the full flow",
            "never guess", // discover-before-use
        ] {
            assert!(
                dl.contains(needle),
                "skeleton description must keep {needle:?}"
            );
        }
        // The worked multi-step example (small models follow examples, not
        // directives — the connector create→enable→test sequence).
        assert!(
            dl.contains("connector create") && dl.contains("connector enable"),
            "worked multi-step example must stay"
        );
    }

    /// Sequence directives: Ling-3.0-tiny's device/rule failures were dominated
    /// by "skip the read step before write" and "don't finish multi-step flows".
    /// Guard the two directives that nudge read-before-write + complete-the-flow.
    #[test]
    fn command_choice_directs_sequence() {
        let d = ShellTool::new(test_config()).description().to_lowercase();
        for needle in ["read before write", "complete the full flow"] {
            assert!(
                d.contains(needle),
                "Command Choice must keep sequence directive {needle:?}"
            );
        }
    }

    #[test]
    fn scheduler_timeout_tracks_inner_config_and_overrides() {
        let tool = ShellTool::new(ShellConfig {
            timeout_secs: 123,
            ..Default::default()
        });
        for (args, expected) in [
            (serde_json::json!({}), 128),
            (serde_json::json!({"timeout":180}), 185),
            (serde_json::json!({"timeout":"180"}), 185),
            (serde_json::json!({"timeout":999}), 605),
        ] {
            assert_eq!(tool.execution_timeout(&args).unwrap().as_secs(), expected);
        }
        assert!(tool
            .execution_timeout(&serde_json::json!({"timeout":"invalid"}))
            .is_err());
        assert!(crate::toolkit::timeouts::shell_default().as_secs() > 70);
    }

    #[tokio::test]
    async fn test_basic_command() {
        let tool = ShellTool::new(test_config());
        let result = tool
            .execute(serde_json::json!({ "command": "echo hello world" }))
            .await
            .unwrap();
        assert!(result.success);
        let data = result.data;
        assert_eq!(data["exit_code"], 0);
        assert!(data["stdout"].as_str().unwrap().contains("hello world"));
        assert_eq!(data["timed_out"], false);
    }

    #[tokio::test]
    async fn test_stderr_capture() {
        let tool = ShellTool::new(test_config());
        let result = tool
            .execute(serde_json::json!({ "command": "echo error >&2" }))
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.data["stderr"].as_str().unwrap().contains("error"));
    }

    #[tokio::test]
    async fn test_nonzero_exit_code() {
        let tool = ShellTool::new(test_config());
        let result = tool
            .execute(serde_json::json!({ "command": "exit 42" }))
            .await
            .unwrap();
        assert!(result.success); // ToolOutput success = tool ran, not command success
        assert_eq!(result.data["exit_code"], 42);
    }

    #[tokio::test]
    async fn test_timeout() {
        let config = ShellConfig {
            enabled: true,
            timeout_secs: 1,
            max_output_chars: 5000,
        };
        let tool = ShellTool::new(config);
        let result = tool
            .execute(serde_json::json!({ "command": "sleep 60" }))
            .await
            .unwrap();
        assert!(result.data["timed_out"].as_bool().unwrap());
        assert!(result.data["stderr"]
            .as_str()
            .unwrap()
            .contains("timed out"));
    }

    #[tokio::test]
    async fn test_per_command_timeout_override() {
        let tool = ShellTool::new(test_config()); // default 10s
        let result = tool
            .execute(serde_json::json!({ "command": "sleep 60", "timeout": 1 }))
            .await
            .unwrap();
        assert!(result.data["timed_out"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_empty_command_rejected() {
        let tool = ShellTool::new(test_config());
        let result = tool.execute(serde_json::json!({ "command": "  " })).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_missing_command_rejected() {
        let tool = ShellTool::new(test_config());
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_working_dir() {
        let tool = ShellTool::new(test_config());
        let result = tool
            .execute(serde_json::json!({ "command": "pwd", "working_dir": "/tmp" }))
            .await
            .unwrap();
        let stdout = result.data["stdout"].as_str().unwrap();
        assert!(stdout.contains("tmp"));
    }

    #[tokio::test]
    async fn test_invalid_working_dir() {
        let tool = ShellTool::new(test_config());
        let result = tool
            .execute(serde_json::json!({
                "command": "pwd",
                "working_dir": "/nonexistent/path"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_pipeline_command() {
        let tool = ShellTool::new(test_config());
        let result = tool
            .execute(serde_json::json!({
                "command": "echo -e 'apple\nbanana\ncherry' | grep an"
            }))
            .await
            .unwrap();
        let stdout = result.data["stdout"].as_str().unwrap();
        assert!(stdout.contains("banana"));
        assert!(!stdout.contains("apple"));
    }

    #[tokio::test]
    async fn test_permission_denied_command() {
        let tool = ShellTool::new(test_config());
        // This should fail with permission error, not crash
        let result = tool
            .execute(serde_json::json!({ "command": "ls /root" }))
            .await
            .unwrap();
        // Tool succeeds (command ran), but exit_code may be non-zero or stderr has error
        assert!(result.success);
        // Either exit_code is non-zero or stderr contains error info
        let exit_code = result.data["exit_code"].as_i64().unwrap_or(0);
        let stderr = result.data["stderr"].as_str().unwrap_or("");
        assert!(exit_code != 0 || !stderr.is_empty() || !result.data["stdout"].is_null());
    }

    #[test]
    fn test_truncate_output_within_budget() {
        let (out, err) = truncate_output("hello", "world", 100);
        assert_eq!(out, "hello");
        assert_eq!(err, "world");
    }

    #[test]
    fn test_truncate_output_exceeds_budget() {
        let stdout = "a".repeat(5000);
        let stderr = "b".repeat(5000);
        let (out, err) = truncate_output(&stdout, &stderr, 1000);
        assert!(out.len() < 1000);
        assert!(err.len() < 1000);
        assert!(out.contains("[truncated"));
        assert!(err.contains("[truncated"));
    }

    #[test]
    fn test_truncate_output_stderr_only() {
        let stdout = "short";
        let stderr = "x".repeat(5000);
        let (out, err) = truncate_output(stdout, &stderr, 1000);
        assert!(err.contains("[truncated"));
        assert!(out.len() + err.len() <= 1200);
    }

    #[test]
    fn test_find_safe_truncation_point_ascii() {
        assert_eq!(find_safe_truncation_point("hello world", 5), 5);
    }

    #[test]
    fn test_find_safe_truncation_point_multibyte() {
        let s = "你好世界";
        let pos = find_safe_truncation_point(s, 4);
        assert_eq!(pos, 3);
        assert!(s.is_char_boundary(pos));
    }

    #[test]
    fn test_tool_name_and_category() {
        let tool = ShellTool::new(test_config());
        assert_eq!(tool.name(), "shell");
        assert!(matches!(tool.category(), ToolCategory::System));
    }

    #[test]
    fn test_silent_success_hint_recognizes_gui_launchers() {
        // macOS / Linux / Windows launchers all fire the hint.
        let hint = ShellTool::silent_success_hint("open /tmp/x.png").unwrap();
        assert!(hint.contains("open"));
        assert!(hint.contains("Do NOT"));

        assert!(ShellTool::silent_success_hint("xdg-open /tmp/x.png").is_some());
        assert!(ShellTool::silent_success_hint("explorer C:\\\\Users").is_some());
        assert!(ShellTool::silent_success_hint("start notepad").is_some());
    }

    #[test]
    fn test_silent_success_hint_strips_env_prefix() {
        // Env-var prefix should not defeat detection.
        let hint = ShellTool::silent_success_hint("DISPLAY=:0 xdg-open /tmp/x.png");
        assert!(hint.is_some());
    }

    #[test]
    fn test_silent_success_hint_ignores_productive_commands() {
        // `mkdir`, `rm`, `touch`, `cd` can legitimately produce no output;
        // we do NOT attach a hint for them — only known GUI launchers.
        assert!(ShellTool::silent_success_hint("mkdir foo").is_none());
        assert!(ShellTool::silent_success_hint("touch /tmp/x").is_none());
        assert!(ShellTool::silent_success_hint("true").is_none());
        assert!(ShellTool::silent_success_hint("").is_none());
    }

    #[test]
    fn test_silent_success_hint_matches_quoted_binary() {
        // Shell quoting shouldn't trip up detection.
        assert!(ShellTool::silent_success_hint("\"open\" /tmp/x.png").is_some());
    }

    // ====================================================================
    // Cancellation / kill-on-drop tests
    // ====================================================================

    /// When the future returned by `ShellTool::execute` is dropped before
    /// completion (the path taken when a `CancellationToken` fires and the
    /// ToolRegistry select! aborts the tool future), the underlying subprocess
    /// MUST be killed — not orphaned.
    ///
    /// This test runs `sleep 30`, drops the execute future after 200ms, then
    /// verifies via `pgrep` that no `sleep` processes remain. If `pgrep` is
    /// unavailable the assertion is skipped (test still passes as a smoke test).
    #[tokio::test]
    async fn test_shell_subprocess_killed_on_future_drop() {
        // Skip on Windows — process-group semantics differ and pgrep may not exist.
        if cfg!(windows) {
            return;
        }

        let tool = ShellTool::new(ShellConfig {
            enabled: true,
            timeout_secs: 30,
            max_output_chars: 10000,
        });

        // Use a unique sleep duration so we can identify our own process.
        // `sleep 30` is the marker.
        let before = count_sleep_30_processes();

        // Box::pin (not tokio::pin!) so we OWN the future and can drop it
        // explicitly. `tokio::pin!` only creates a Pin<&mut T> reference,
        // so `drop()` on it drops the reference, not the underlying future —
        // leaving the subprocess alive.
        let mut boxed = Box::pin(tool.execute(serde_json::json!({"command": "sleep 30"})));

        // Poll for 200ms — should not complete (sleep runs 30s).
        let poll_result =
            tokio::time::timeout(std::time::Duration::from_millis(200), boxed.as_mut()).await;
        assert!(
            poll_result.is_err(),
            "sleep 30 should not have finished in 200ms"
        );

        // Drop the boxed future — simulates the ToolRegistry select! cancelling it.
        // This MUST trigger SubprocessGuard::drop, killing the subprocess.
        drop(boxed);

        // Give the OS a moment to reap the killed process group.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let after = count_sleep_30_processes();
        assert!(
            after <= before,
            "sleep 30 process should be killed on future drop; before={}, after={}",
            before,
            after
        );
    }

    /// Count `sleep 30` processes currently running. Uses `pgrep -f 'sleep 30'`
    /// (portable across BSD/macOS and Linux — neither supports `-c` consistently).
    /// Returns 0 if pgrep is unavailable (test assertion becomes permissive).
    fn count_sleep_30_processes() -> usize {
        let out = std::process::Command::new("pgrep")
            .arg("-f")
            .arg("sleep 30")
            .output();
        match out {
            Ok(o) => {
                let s = String::from_utf8_lossy(&o.stdout);
                if s.trim().is_empty() {
                    0
                } else {
                    s.lines().count()
                }
            }
            Err(_) => 0, // pgrep unavailable; assertion becomes permissive
        }
    }
}

/// Edge coverage for the command tokenizer and output truncation — the
/// in-process dispatch path the agent uses for every `neomind` invocation.
/// A tokenizer bug here either crashes the tool call (panic) or silently
/// rewrites the agent's command; a truncation bug can panic on CJK output
/// (byte/char boundary mismatch) or lose the entire stderr.
#[cfg(test)]
mod dispatch_edge_tests {
    use super::*;

    #[test]
    fn tokenizer_handles_quotes_and_escapes() {
        // Single-quoted blob keeps spaces and double quotes intact.
        let toks =
            tokenize_neomind_command(r#"agent send-message a1 'hello "world" now'"#).unwrap();
        assert_eq!(toks, ["agent", "send-message", "a1", "hello \"world\" now"]);

        // Double quotes + escaped quote inside.
        let toks = tokenize_neomind_command(r#"message send "say \"hi\"""#).unwrap();
        assert_eq!(toks, ["message", "send", "say \"hi\""]);

        // Escaped space glues two words into one argument.
        let toks = tokenize_neomind_command(r"device get my\ device").unwrap();
        assert_eq!(toks, ["device", "get", "my device"]);

        // CJK passes through as ordinary argument characters.
        let toks = tokenize_neomind_command("rule create --name 温湿度告警").unwrap();
        assert_eq!(toks, ["rule", "create", "--name", "温湿度告警"]);
    }

    /// NM-254 + P0-1: `device get` `shell_example` is a structured
    /// `{command, params}` object that survives shell structured `params`
    /// injection (and the terminal CLI `--params '<JSON>'` form still survives
    /// verbatim tokenization) and delivers byte-identical text (CJK + spaces + `'`)
    /// to the device adapter exactly once.
    #[tokio::test]
    async fn nm254_device_shell_example_cjk_space_quote_roundtrips_to_adapter() {
        use clap::Parser;
        use futures::Stream;
        use neomind_cli_ops::device::{device_control_cli_example, device_control_shell_example};
        use neomind_cli_ops::dispatch::commands::{Args, Command, DeviceCommand};
        use neomind_core::EventBus;
        use neomind_devices::mdl::MetricDataType;
        use neomind_devices::mdl_format::{CommandDefinition, ParameterDefinition};
        use neomind_devices::{
            AdapterResult, ConnectionConfig, ConnectionStatus, DeviceAdapter, DeviceConfig,
            DeviceEvent, DeviceRegistry, DeviceService, DeviceTypeTemplate,
        };
        use std::pin::Pin;
        use std::sync::Arc;

        let text = "你好 it's a test";
        let params = serde_json::json!({"text": text});
        let example = device_control_shell_example("eink-nm254", "render_text", &params)
            .expect("reliable object params must yield a shell_example");
        validate_extension_params_sources(&example)
            .expect("structured shell_example must pass shell validation");

        // Route structured {command, params} through host-context argv injection.
        let cmd_str = example["command"].as_str().expect("command string");
        let base_argv = tokenize_neomind_command(cmd_str).expect("command must tokenize");
        let host_ctx = extension_invoke_host_context_from_args(&example).expect("host ctx");
        let injected_argv =
            neomind_cli_ops::dispatch::with_extension_invoke_host_context(host_ctx, async {
                inject_structured_json_argv(base_argv)
            })
            .await
            .expect("structured params injection");

        // Terminal CLI string form must also tokenize + parse cleanly.
        let cli_example =
            device_control_cli_example("eink-nm254", "render_text", &params).expect("CLI example");
        let cli_argv = tokenize_neomind_command(&cli_example).expect("CLI example must tokenize");

        let mut parsed_params_json = serde_json::Value::Null;
        let mut parsed_id = String::new();
        let mut parsed_command = String::new();
        for argv in [injected_argv, cli_argv] {
            let parsed = Args::try_parse_from(argv.iter()).expect("example must clap-parse");
            let Command::Device { device_cmd } = parsed.command else {
                panic!("expected device command from shell_example");
            };
            let DeviceCommand::Control {
                id,
                command,
                params: params_flag,
                param,
            } = device_cmd
            else {
                panic!("expected device control from shell_example");
            };
            assert!(
                param.is_empty(),
                "shell_example uses --params, not repeated --param"
            );
            let pj: serde_json::Value = serde_json::from_str(
                params_flag
                    .as_deref()
                    .expect("--params must be present on shell_example"),
            )
            .expect("--params JSON");
            assert_eq!(
                pj["text"].as_str(),
                Some(text),
                "tokenizer+clap must preserve text byte/char-identical"
            );
            assert_eq!(id, "eink-nm254");
            assert_eq!(command, "render_text");
            parsed_params_json = pj;
            parsed_id = id;
            parsed_command = command;
        }
        let params_json = parsed_params_json;
        let id = parsed_id;
        let command = parsed_command;

        struct RecordingAdapter {
            sent: Arc<tokio::sync::Mutex<Vec<(String, String, String)>>>,
        }
        #[async_trait::async_trait]
        impl DeviceAdapter for RecordingAdapter {
            fn name(&self) -> &str {
                "nm254-rec"
            }
            fn adapter_type(&self) -> &'static str {
                "test"
            }
            fn is_running(&self) -> bool {
                true
            }
            async fn start(&self) -> AdapterResult<()> {
                Ok(())
            }
            async fn stop(&self) -> AdapterResult<()> {
                Ok(())
            }
            fn subscribe(&self) -> Pin<Box<dyn Stream<Item = DeviceEvent> + Send + '_>> {
                Box::pin(futures::stream::empty())
            }
            fn device_count(&self) -> usize {
                0
            }
            fn list_devices(&self) -> Vec<String> {
                Vec::new()
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            async fn send_command(
                &self,
                device_id: &str,
                command_name: &str,
                payload: String,
                _topic: Option<String>,
            ) -> AdapterResult<()> {
                self.sent.lock().await.push((
                    device_id.to_string(),
                    command_name.to_string(),
                    payload,
                ));
                Ok(())
            }
            fn connection_status(&self) -> ConnectionStatus {
                ConnectionStatus::Connected
            }
            async fn subscribe_device(&self, _device_id: &str) -> AdapterResult<()> {
                Ok(())
            }
            async fn unsubscribe_device(&self, _device_id: &str) -> AdapterResult<()> {
                Ok(())
            }
        }

        let registry = Arc::new(DeviceRegistry::new());
        let service = Arc::new(DeviceService::new(registry, EventBus::new()));
        let template =
            DeviceTypeTemplate::new("nm254_eink", "NM254 Eink").with_command(CommandDefinition {
                name: "render_text".into(),
                display_name: "Render Text".into(),
                payload_template: r#"{"text":"${text}"}"#.into(),
                parameters: vec![ParameterDefinition {
                    name: "text".into(),
                    display_name: "Text".into(),
                    data_type: MetricDataType::String,
                    unit: String::new(),
                    min: None,
                    max: None,
                    default_value: None,
                    allowed_values: vec![],
                    required: true,
                    visible_when: None,
                    group: None,
                    help_text: String::new(),
                    validation: vec![],
                }],
                fixed_values: Default::default(),
                samples: vec![],
                description: String::new(),
                parameter_groups: vec![],
            });
        service.register_template(template).await.unwrap();
        service
            .register_device(DeviceConfig {
                device_id: id.clone(),
                name: "NM254".into(),
                device_type: "nm254_eink".into(),
                adapter_type: "test".into(),
                connection_config: ConnectionConfig::new(),
                adapter_id: Some("nm254-rec".into()),
                last_seen: 0,
                offline_timeout_secs: None,
            })
            .await
            .unwrap();
        let adapter = Arc::new(RecordingAdapter {
            sent: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        });
        service
            .register_adapter(
                "nm254-rec".into(),
                adapter.clone() as Arc<dyn DeviceAdapter>,
            )
            .await;

        let mut send_params = std::collections::HashMap::new();
        for (k, v) in params_json.as_object().unwrap() {
            send_params.insert(k.clone(), v.clone());
        }
        service
            .send_command(&id, &command, send_params)
            .await
            .expect("valid params from shell_example must reach the adapter");

        let sent = adapter.sent.lock().await.clone();
        assert_eq!(sent.len(), 1, "adapter must receive exactly once: {sent:?}");
        assert_eq!(sent[0].0, id);
        assert_eq!(sent[0].1, "render_text");
        let payload: serde_json::Value = serde_json::from_str(&sent[0].2).expect("payload is JSON");
        assert_eq!(
            payload["text"].as_str(),
            Some(text),
            "adapter payload text must match shell_example params byte/char-identical"
        );
    }

    #[test]
    fn split_path_hint_targets_the_offending_argument() {
        let case = |msg: &str, command: &str| {
            should_hint_split_path(msg, &tokenize_neomind_command(command).unwrap())
        };
        let error = "unexpected argument 'Support/com.neomind.neomind/data/frontend-components/device-info-widget' found";
        assert!(case(
            error,
            "neomind widget install /Users/x/Library/Application Support/com.neomind.neomind/data/frontend-components/device-info-widget",
        ));
        // Quotes around another argument do not repair the split path.
        assert!(case(error,
            "neomind widget install /Users/x/Library/Application Support/com.neomind.neomind/data/frontend-components/device-info-widget --label \"two words\""));
        // Quoted and escaped paths remain a single parsed argument.
        assert!(!case(error,
            "neomind widget install \"/Users/x/Library/Application Support/com.neomind.neomind/data/frontend-components/device-info-widget\""));
        assert!(!case(
            error,
            r"neomind widget install /Users/x/Library/Application\ Support/com.neomind.neomind/data/frontend-components/device-info-widget"
        ));
        for flag in ["--components", "--id", "--url"] {
            assert!(!case(
                &format!("unexpected argument '{flag}' found"),
                &format!("neomind widget install /tmp/widget {flag} value"),
            ));
        }
        assert!(!case(
            "unexpected argument 'foo' found",
            "neomind widget install /tmp/widget foo",
        ));
        assert!(!case(
            "unexpected argument '/tmp/second' found",
            "neomind widget install /tmp/first /tmp/second"
        ));
        assert!(!case(
            "missing required arguments",
            "neomind widget install"
        ));
    }

    #[tokio::test]
    async fn real_clap_errors_do_not_misdiagnose_unsupported_flags_as_paths() {
        for command in [
            "neomind dashboard update-component dash-1 --components '{}'",
            "neomind transform update --id transform-1 --code 'return input'",
            "neomind connector create --name test --url https://example.com",
        ] {
            let argv = tokenize_neomind_command(command).unwrap();
            let error = neomind_cli_ops::dispatch::dispatch(&argv)
                .await
                .expect_err("invalid flags must remain rejected");
            let neomind_cli_ops::dispatch::DispatchError::Parse(message) = error else {
                panic!("expected clap parse failure for {command}");
            };
            assert!(message.contains("unexpected argument"), "{message}");
            assert!(!should_hint_split_path(&message, &argv));
        }
        let argv = tokenize_neomind_command(
            "neomind widget install /Users/x/Library/Application Support/widget",
        )
        .unwrap();
        let neomind_cli_ops::dispatch::DispatchError::Parse(message) =
            neomind_cli_ops::dispatch::dispatch(&argv)
                .await
                .expect_err("split path")
        else {
            panic!("expected split-path parse error");
        };
        assert!(should_hint_split_path(&message, &argv), "{message}");
    }

    #[test]
    fn tokenizer_rejects_shell_constructs_outside_quotes() {
        for bad in [
            "device list | grep x",
            "device list > out.txt",
            "device list < in.txt",
            "device list `date`",
            "device list $HOME",
        ] {
            assert!(
                tokenize_neomind_command(bad).is_err(),
                "must reject shell construct: {bad}"
            );
        }
    }

    #[test]
    fn tokenizer_accepts_shell_chars_inside_quotes() {
        // The same constructs are fine when quoted — they become literal
        // argument content, which clap receives intact.
        let toks = tokenize_neomind_command(r"message send 'a | b > c $d'").unwrap();
        assert_eq!(toks, ["message", "send", "a | b > c $d"]);
    }

    #[test]
    fn readonly_list_models_quoted_subst_stays_literal_argv_not_shell() {
        // Classifier: list-models is ProvenReadOnly. Tokenize keeps quoted
        // `$(…)` literal. NotInProcess must spawn that argv — re-shelling the
        // raw line would execute the substitution.
        // Double-quoted `$()` survives tokenize as a literal arg; /bin/sh -c
        // would expand it. Use r## so the closing `"#` of the endpoint does
        // not terminate the Rust raw string.
        let cmd = r##"neomind list-models --endpoint "$(curl http://evil.example/)""##;
        let argv = tokenize_neomind_command(cmd).expect("quoted $() tokenizes");
        assert_eq!(argv[0], "neomind");
        assert_eq!(argv[1], "list-models");
        assert!(
            argv.iter()
                .any(|a| a.contains("$(curl http://evil.example/)")),
            "substitution must remain literal in argv: {argv:?}"
        );
        let cap = classify_shell_capability(cmd);
        assert_eq!(
            cap.retry_policy,
            crate::agent::task_continuity::RetryPolicy::AlwaysSafe,
            "list-models must classify as read-only"
        );
        assert!(
            not_in_process_uses_parsed_argv(),
            "NotInProcess after readonly classify must not re-interpret raw shell"
        );
    }

    #[test]
    fn tokenizer_rejects_unbalanced_quotes() {
        assert!(tokenize_neomind_command("message send 'unclosed").is_err());
        assert!(tokenize_neomind_command(r#"message send "unclosed"#).is_err());
    }

    /// Regression for the CJK crash class: budgets are in BYTES while
    /// content is often multi-byte — truncation must back off to a char
    /// boundary, never slice mid-codepoint (would panic the tool call).
    #[test]
    fn truncation_never_splits_multibyte_chars() {
        let stdout = "温".repeat(2000); // 3 bytes each, 6000 bytes total
        let stderr = "";
        let (out, err) = truncate_output(&stdout, stderr, 300);
        assert!(out.contains("truncated"), "must mark truncation: {out}");
        assert!(!out.is_empty() && !err.is_empty() || err.is_empty());
        // The kept prefix must be valid (test would have panicked otherwise)
        // and the notice must report the byte count actually omitted.
        assert!(out.contains("chars omitted"));
    }

    #[test]
    fn truncation_splits_budget_between_streams() {
        // Both streams over budget: each keeps a proportional share and
        // gets its own notice; stderr must not be dropped wholesale.
        let stdout = "S".repeat(1000);
        let stderr = "E".repeat(1000);
        let (out, err) = truncate_output(&stdout, &stderr, 400);
        assert!(out.contains("truncated") && out.contains('S'));
        assert!(err.contains("truncated") && err.contains('E'));
        assert!(out.matches('S').count() < 1000);
        assert!(err.matches('E').count() < 1000);
    }

    #[test]
    fn truncation_passes_small_output_through_untouched() {
        let (out, err) = truncate_output("ok", "warn", 100);
        assert_eq!((out.as_str(), err.as_str()), ("ok", "warn"));
    }

    #[test]
    fn safe_truncation_point_backs_off_to_boundary() {
        // "温" is 3 bytes; max_bytes 4 lands mid-char and must back to 3.
        let s = "温温温";
        assert_eq!(find_safe_truncation_point(s, 4), 3);
        assert_eq!(find_safe_truncation_point(s, 6), 6);
        assert_eq!(find_safe_truncation_point(s, 999), s.len());
        assert_eq!(find_safe_truncation_point("", 10), 0);
    }

    /// The domain index in the tool description must match the CLI it describes.
    ///
    /// That index is the model's map of what exists: `neomind <domain> --help`
    /// gives flags, but this line decides which domains and subcommands it even
    /// considers. Nothing compared the two, which is how `--auto-approve` stayed
    /// in a skill after 8ec68e2c deleted the flag, and how a subcommand could be
    /// renamed and simply vanish from the model's world.
    #[test]
    fn the_domain_index_lists_exactly_what_the_cli_has() {
        use clap::CommandFactory;

        let root = neomind_cli_ops::dispatch::commands::Args::command();
        let mut problems: Vec<String> = Vec::new();
        let mut domains_seen = 0usize;

        for line in DOMAIN_INDEX.lines() {
            let Some(rest) = line.strip_prefix("- ") else {
                continue;
            };
            let Some((domain, listed)) = rest.split_once(": ") else {
                continue;
            };
            let Some(cmd) = root.get_subcommands().find(|c| c.get_name() == domain) else {
                problems.push(format!(
                    "`{domain}` is listed but the CLI has no such domain"
                ));
                continue;
            };
            domains_seen += 1;

            // The line is `- <domain>: a b c — <sub>: d e`, where the part after
            // the em dash names a nested group's own subcommands. Check each
            // level against the level it belongs to.
            // Strip parenthetical asides FIRST: a connector line's aside contains
            // its own em dash and colon, and splitting before removing it turned
            // "MQTT broker / webhook / HTTP — NOT devices" into a nested group.
            let listed = listed.split('(').next().unwrap_or(listed);
            let segments: Vec<&str> = listed.split('—').map(str::trim).collect();
            for (level, segment) in segments.iter().enumerate() {
                let (parent, names) = match segment.split_once(':') {
                    Some((sub, names)) => (sub.trim(), names),
                    None => (domain, *segment),
                };
                let owner = if level == 0 {
                    cmd
                } else {
                    match cmd.get_subcommands().find(|c| c.get_name() == parent) {
                        Some(c) => c,
                        None => {
                            problems
                                .push(format!("`{domain} {parent}` is named but does not exist"));
                            continue;
                        }
                    }
                };
                let real_here: std::collections::BTreeSet<&str> =
                    owner.get_subcommands().map(|c| c.get_name()).collect();
                // `<ID>` and friends are arguments, not commands.
                let named: std::collections::BTreeSet<&str> = names
                    .split_whitespace()
                    .filter(|t| {
                        !t.starts_with('<')
                            && !t.starts_with('`')
                            && t.chars().all(|c| c.is_ascii_lowercase() || c == '-')
                    })
                    .collect();
                let where_ = if level == 0 {
                    domain.to_string()
                } else {
                    format!("{domain} {parent}")
                };
                for ghost in named.difference(&real_here) {
                    problems.push(format!("`{where_} {ghost}` is listed but does not exist"));
                }
                for missing in real_here.difference(&named) {
                    problems.push(format!(
                        "`{where_} {missing}` exists but is not in the index, so the model \
                         will not know to use it"
                    ));
                }
            }
        }

        // The other direction: a whole domain the model never learns about. The
        // index carries its own exclusion list — commands that only run as a
        // separate process, or that an agent must not run at all — so this also
        // checks that a domain is either offered or explicitly ruled out.
        let indexed: std::collections::BTreeSet<&str> = DOMAIN_INDEX
            .lines()
            .filter_map(|l| l.strip_prefix("- "))
            .filter_map(|l| l.split(':').next())
            .collect();
        let excluded: std::collections::BTreeSet<&str> = DOMAIN_INDEX
            .split("Deliberately not available here")
            .nth(1)
            .and_then(|rest| rest.split('—').next())
            .unwrap_or("")
            .split_whitespace()
            .collect();
        for cmd in root.get_subcommands() {
            let name = cmd.get_name();
            if name == "help" || indexed.contains(name) || excluded.contains(name) {
                continue;
            }
            problems.push(format!(
                "domain `{name}` is neither in the index nor in the exclusion list — the model \
                 has no way to know whether it can use it"
            ));
        }

        assert!(
            domains_seen > 8,
            "parsed only {domains_seen} domains — the scan is broken"
        );
        assert!(
            problems.is_empty(),
            "domain index drift:\n  {}",
            problems.join("\n  ")
        );
    }

    /// Platform message center vs device control must stay distinct in the index.
    ///
    /// Models otherwise treat `message send` as "put text on this device" (e.g.
    /// `@INK 发送一段文本`). The aside must steer them to `device get` →
    /// `device control` for any device, and keep the agents note.
    #[test]
    fn message_index_steers_device_actions_to_device_control() {
        let message_line = DOMAIN_INDEX
            .lines()
            .find(|l| l.starts_with("- message:"))
            .expect("message domain line");
        assert!(message_line.contains("`message send` creates a platform message"));
        assert!(message_line.contains("NOT for talking to agents"));
        assert!(message_line.contains("device get <id>"));
        assert!(message_line.contains("device control <id> <command>"));
        assert!(
            !message_line.to_ascii_lowercase().contains("eink"),
            "device routing must stay device-generic, not e-ink-specific"
        );
    }

    /// NM-249: domain index footer is a cheat sheet, not a completeness claim.
    #[test]
    fn nm249_domain_index_footer_is_cheat_sheet() {
        assert!(DOMAIN_INDEX.contains("common cheat sheet"));
        assert!(DOMAIN_INDEX.contains("full flags and aliases"));
        assert!(!DOMAIN_INDEX.contains("Anything not listed above does not exist"));
    }

    /// NM-252: overview via list; concrete task via `--search` + shell_example;
    /// no residual sanitized-name example.
    #[test]
    fn nm249_extension_selection_rules_in_description() {
        let tool = ShellTool::new(ShellConfig::default());
        let d = tool.description();
        assert!(d.contains("extension list"));
        assert!(d.contains("extension commands --search"));
        assert!(d.contains("shell_example"));
        assert!(d.contains("extension commands <ID>"));
        assert!(!d.contains("weather-forecast-v2_get_weather"));
        assert!(!d.contains("--params '{...}'"));
    }

    /// Every `neomind ...` command the builtin skills show the model must parse.
    ///
    /// The skills are what the model copies from. `25a908f5` fixed five examples
    /// whose syntax had drifted from the real commands — each had been steering
    /// a retry into a second failure — and nothing was comparing them, so this
    /// does: pull the command lines out of the fenced blocks and hand them to
    /// the same clap definition the CLI runs.
    ///
    /// Lines the in-process tokenizer refuses (pipes, redirections, `$`,
    /// backticks) are real shell lines, not `neomind` invocations, and are
    /// skipped by the same rule the runtime uses.
    #[test]
    fn every_command_the_skills_show_the_model_parses() {
        use clap::Parser;

        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/skills/builtins");
        let mut checked = 0usize;
        let mut failures: Vec<String> = Vec::new();

        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .expect("skills dir")
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "md"))
            .collect();
        files.sort();

        for path in files {
            let skill = path.file_stem().unwrap().to_string_lossy().to_string();
            let text = std::fs::read_to_string(&path).expect("read skill");
            let lines: Vec<&str> = text.lines().collect();
            let mut i = 0usize;
            while i < lines.len() {
                let start_line = i + 1;
                // A recipe may be wrapped: `neomind agent create \` and the
                // flags on the following lines. Join them back into one.
                let mut full = String::new();
                loop {
                    let piece = lines[i].trim();
                    let continues = piece.ends_with('\\');
                    let piece = piece.trim_end_matches('\\').trim();
                    if full.is_empty() {
                        full.push_str(piece);
                    } else {
                        full.push(' ');
                        full.push_str(piece);
                    }
                    i += 1;
                    if !continues || i >= lines.len() {
                        break;
                    }
                }
                // Drop a trailing `# comment`, but only when the `#` is outside
                // quotes — a `#` inside a prompt is text.
                if let Some(at) = full.find(" #") {
                    if full[..at].matches('"').count().is_multiple_of(2) {
                        full.truncate(at);
                    }
                }
                let full = full.trim().to_string();
                if !full.starts_with("neomind ") {
                    continue;
                }
                if full.contains('<') || full.contains("...") || full.contains("neomind x") {
                    continue;
                }
                let Ok(argv) = tokenize_neomind_command(&full) else {
                    continue; // a shell line, by the runtime's own rule
                };
                checked += 1;
                if let Err(e) = neomind_cli_ops::dispatch::commands::Args::try_parse_from(&argv) {
                    failures.push(format!("{skill}.md:{start_line}  {full}\n      -> {e}"));
                }
            }
        }

        assert!(
            checked > 50,
            "only found {checked} commands — the scan is not reading the skills"
        );
        assert!(
            failures.is_empty(),
            "{} skill example(s) do not parse:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}
#[cfg(test)]
mod nm247_structured_params_tests {
    use super::{
        bind_extension_invoke_prepared_token, classify_shell_capability_with_args,
        extension_invoke_host_context_from_args, inject_structured_json_argv,
        strip_host_meta_from_argv, validate_extension_params_sources,
    };
    use neomind_core::extension::{
        begin_instance_generation, classify_and_bind_chat_invoke, invalidate_instance_capability,
        CommandEffect, ExtensionCommand, ExtensionCommandSnapshot, MetricDataType,
        ParameterDefinition,
    };
    use serde_json::json;

    fn publish(id: &str) {
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
        ExtensionCommandSnapshot::build(id, "2.14.0", None, vec![get], gen)
            .publish_capabilities()
            .unwrap();
    }

    #[test]
    fn strip_host_meta_drops_new_operation_and_prepared_tokens() {
        let argv = vec![
            "neomind".into(),
            "dashboard".into(),
            "update-component".into(),
            "d1".into(),
            "--component-id".into(),
            "c1".into(),
            "--set".into(),
            r#"{"position":{"w":2,"h":2}}"#.into(),
            "_new_operation=true".into(),
            "--prepared-operation-id".into(),
            "op_dead".into(),
        ];
        let cleaned = strip_host_meta_from_argv(argv);
        assert_eq!(
            cleaned,
            vec![
                "neomind",
                "dashboard",
                "update-component",
                "d1",
                "--component-id",
                "c1",
                "--set",
                r#"{"position":{"w":2,"h":2}}"#,
            ]
        );
    }

    #[test]
    fn conflict_params_object_and_set_flag_rejected_for_dashboard() {
        let args = json!({
            "command": "neomind dashboard update-component d1 --component-id c1 --set {\"w\":2}",
            "params": {"position":{"w":2,"h":2}}
        });
        let err = validate_extension_params_sources(&args).unwrap_err();
        assert!(err.contains("conflict"), "{err}");
        assert!(err.contains("--set"), "{err}");
    }

    #[test]
    fn conflict_params_object_and_cli_flag_rejected() {
        let args = json!({
            "command": "neomind extension invoke weather-forecast-v2 get_weather --params {\"city\":\"Xiamen\"}",
            "params": {"city": "Xiamen"}
        });
        let err = validate_extension_params_sources(&args).unwrap_err();
        assert!(err.contains("conflict"), "{err}");
    }

    #[test]
    fn non_object_params_rejected() {
        let args = json!({
            "command": "neomind extension invoke weather-forecast-v2 get_weather",
            "params": "Xiamen"
        });
        let err = validate_extension_params_sources(&args).unwrap_err();
        assert!(err.contains("object"), "{err}");
    }

    #[test]
    fn non_extension_shell_ignores_params_field() {
        let args = json!({
            "command": "neomind device list",
            "params": {"city": "Xiamen"}
        });
        assert!(validate_extension_params_sources(&args).is_ok());
    }

    #[tokio::test]
    async fn array_params_accepted_and_injected_for_dashboard_array_flags() {
        let add_args = json!({
            "command": "neomind dashboard add-components d1",
            "params": [{"id": "c1", "type": "value-card", "position": {"x":0,"y":0,"w":4,"h":2}}]
        });
        validate_extension_params_sources(&add_args)
            .expect("array params must be accepted for dashboard add-components");

        let rem_args = json!({
            "command": "neomind dashboard remove-components d1",
            "params": ["c1", "c2"]
        });
        validate_extension_params_sources(&rem_args)
            .expect("array params must be accepted for dashboard remove-components");

        // Wrong container kind must be rejected cleanly.
        let wrong_obj_for_add = json!({
            "command": "neomind dashboard add-components d1",
            "params": {"id": "c1"}
        });
        let err = validate_extension_params_sources(&wrong_obj_for_add).unwrap_err();
        assert!(err.contains("array"), "{err}");

        let wrong_arr_for_set = json!({
            "command": "neomind dashboard update-component d1 --component-id c1",
            "params": ["c1"]
        });
        let err = validate_extension_params_sources(&wrong_arr_for_set).unwrap_err();
        assert!(err.contains("object"), "{err}");

        // Host context + argv injection preserves array JSON cleanly.
        let ctx = extension_invoke_host_context_from_args(&rem_args).expect("host ctx");
        let argv = vec![
            "neomind".into(),
            "dashboard".into(),
            "remove-components".into(),
            "d1".into(),
        ];
        let out = neomind_cli_ops::dispatch::with_extension_invoke_host_context(ctx, async {
            inject_structured_json_argv(argv)
        })
        .await
        .expect("inject array params");
        assert_eq!(
            out,
            vec![
                "neomind",
                "dashboard",
                "remove-components",
                "d1",
                "--ids",
                r#"["c1","c2"]"#,
            ]
        );
    }

    #[test]
    fn classify_shell_strips_host_meta_and_respects_structured_params() {
        // Read-only query with leaked _new_operation inside command must stay read_query
        // (never downgraded to mutating_unknown_sensitive).
        let cap = classify_shell_capability_with_args(
            "neomind device get cam-1 _new_operation=true",
            &json!({}),
        );
        assert!(
            cap.read_only,
            "leaked _new_operation must not downgrade RO: {cap:?}"
        );

        let cap_logs = classify_shell_capability_with_args(
            "neomind extension logs weather-forecast-v2 --_new_operation",
            &json!({}),
        );
        assert!(
            cap_logs.read_only,
            "extension logs must be RO: {cap_logs:?}"
        );

        let cap_push_logs =
            classify_shell_capability_with_args("neomind push logs p1 --limit 10", &json!({}));
        assert!(
            cap_push_logs.read_only,
            "push logs must be RO: {cap_push_logs:?}"
        );

        // extension config without --set/params is read-only; WITH structured params is Mutating.
        let cfg_read = classify_shell_capability_with_args(
            "neomind extension config weather-forecast-v2",
            &json!({}),
        );
        assert!(
            cfg_read.read_only,
            "extension config read must be RO: {cfg_read:?}"
        );

        let cfg_write = classify_shell_capability_with_args(
            "neomind extension config weather-forecast-v2",
            &json!({"params": {"city": "Beijing"}}),
        );
        assert!(
            !cfg_write.read_only,
            "extension config with params must be Mutating: {cfg_write:?}"
        );
    }

    #[tokio::test]
    async fn host_ticket_bind_without_argv_token() {
        let id = "weather-nm247-host-ticket";
        publish(id);
        let params = json!({"city": "Xiamen"});
        let bind = classify_and_bind_chat_invoke(id, "get_weather", &params);
        let op = bind.prepared_operation_id.expect("ticket");
        let argv = vec![
            "neomind".into(),
            "extension".into(),
            "invoke".into(),
            id.into(),
            "get_weather".into(),
        ];
        neomind_cli_ops::dispatch::with_extension_invoke_host_context(
            neomind_cli_ops::dispatch::ExtensionInvokeHostContext {
                prepared_operation_id: Some(op),
                structured_params: Some(params),
            },
            async {
                bind_extension_invoke_prepared_token(argv).expect("host ticket must bind");
            },
        )
        .await;
        invalidate_instance_capability(id);
    }

    #[tokio::test]
    async fn invented_argv_token_ignored_when_host_ticket_present() {
        let id = "weather-nm247-host-wins";
        publish(id);
        let params = json!({"city": "Beijing"});
        let bind = classify_and_bind_chat_invoke(id, "get_weather", &params);
        let op = bind.prepared_operation_id.expect("ticket");
        let argv = vec![
            "neomind".into(),
            "extension".into(),
            "invoke".into(),
            id.into(),
            "get_weather".into(),
            "--prepared-operation-id".into(),
            "op_fresh_weather_2026".into(),
        ];
        neomind_cli_ops::dispatch::with_extension_invoke_host_context(
            neomind_cli_ops::dispatch::ExtensionInvokeHostContext {
                prepared_operation_id: Some(op),
                structured_params: Some(params),
            },
            async {
                bind_extension_invoke_prepared_token(argv).expect("host wins over invented token");
            },
        )
        .await;
        invalidate_instance_capability(id);
    }

    /// Case-c: attach mints ticket → canonicalize strips for fp → restore →
    /// host context → bind claims (no PreparedInvoke missing).
    #[tokio::test]
    async fn case_c_followup_ticket_survives_execute_canonicalize() {
        let id = "weather-nm247-case-c";
        publish(id);
        let params = json!({"city": "上海"});
        let bind = classify_and_bind_chat_invoke(id, "get_weather", &params);
        let op = bind.prepared_operation_id.expect("classify must mint");
        let mut args = json!({
            "command": format!(
                "neomind extension invoke {id} get_weather --params '{{\"city\":\"上海\"}}'"
            ),
            crate::agent::task_continuity::HOST_PREPARED_OPERATION_ARG: op.clone(),
        });
        // Simulate execute_with_retry_impl canonicalize + restore.
        let mut canon = crate::agent::task_continuity::canonicalize_tool_arguments("shell", &args);
        crate::agent::task_continuity::restore_host_prepared_carriage(&args, &mut canon);
        args = canon;
        let argv = vec![
            "neomind".into(),
            "extension".into(),
            "invoke".into(),
            id.into(),
            "get_weather".into(),
            "--params".into(),
            r#"{"city":"上海"}"#.into(),
        ];
        neomind_cli_ops::dispatch::with_extension_invoke_host_context(
            neomind_cli_ops::dispatch::ExtensionInvokeHostContext {
                prepared_operation_id: args
                    .get(crate::agent::task_continuity::HOST_PREPARED_OPERATION_ARG)
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                structured_params: Some(params),
            },
            async {
                bind_extension_invoke_prepared_token(argv)
                    .expect("restored host ticket must claim once");
            },
        )
        .await;
        // Second claim of same id must fail (single-claim).
        let err = neomind_core::extension::claim_prepared_by_id(&op).unwrap_err();
        assert!(
            err.contains("already claimed") || err.contains("consumed"),
            "ticket must be claimed exactly once: {err}"
        );
        invalidate_instance_capability(id);
    }

    #[test]
    fn transport_connect_refuse_is_not_submitted() {
        let msg = "http_connect: error sending request for url (http://localhost:9375/api/extensions/weather/invoke): connection refused";
        let out = super::classify_extension_transport_error(msg);
        assert!(out.contains("not submitted"), "{out}");
        assert!(out.contains("not reached"), "{out}");
        assert!(!out.contains("result unknown"), "{out}");
    }

    #[test]
    fn transport_timeout_is_result_unknown_no_auto_resend() {
        let msg = "http_timeout: error sending request for url (http://localhost:9375/api/extensions/weather/invoke): operation timed out";
        let out = super::classify_extension_transport_error(msg);
        assert!(out.contains("result unknown"), "{out}");
        assert!(!out.contains("not reached"), "{out}");
        assert!(!out.contains("not submitted"), "{out}");
        assert!(out.contains("do not auto-resend"), "{out}");
        assert!(out.contains("ledger"), "{out}");
    }
}

/// NM-240: shell bind carries classify-time id; no params FIFO claim.
#[cfg(test)]
mod prepared_invoke_bind_tests {
    use super::bind_extension_invoke_prepared_token;
    use neomind_core::extension::{
        begin_instance_generation, classify_and_bind_chat_invoke, invalidate_instance_capability,
        peek_prepared_invoke, CommandEffect, ExtensionCommand, ExtensionCommandSnapshot,
        MetricDataType, ParameterDefinition,
    };
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn publish(id: &str, effect: CommandEffect) {
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
            effect: Some(effect),
            replay_safety: None,
            execution: None,
            ..Default::default()
        };
        ExtensionCommandSnapshot::build(id, "2.14.0", None, vec![get], gen)
            .publish_capabilities()
            .unwrap();
    }

    #[tokio::test]
    async fn shell_bind_after_ro_classify_reload_mutating_is_stale() {
        let id = "weather-forecast-shell-bind-v1";
        publish(id, CommandEffect::ReadOnly);
        let params = json!({"city": "Xiamen"});
        let bind = classify_and_bind_chat_invoke(id, "get_weather", &params);
        assert_eq!(bind.effect, CommandEffect::ReadOnly);
        let op = bind.prepared_operation_id.unwrap();

        publish(id, CommandEffect::Mutating);

        let argv = vec![
            "neomind".into(),
            "extension".into(),
            "invoke".into(),
            id.into(),
            "get_weather".into(),
            "--params".into(),
            r#"{"city":"Xiamen"}"#.into(),
        ];
        let err = neomind_cli_ops::dispatch::with_extension_invoke_host_context(
            neomind_cli_ops::dispatch::ExtensionInvokeHostContext {
                prepared_operation_id: Some(op),
                structured_params: Some(params.clone()),
            },
            async { bind_extension_invoke_prepared_token(argv) },
        )
        .await
        .unwrap_err();
        assert!(
            err.contains("stale")
                || err.contains("unbound")
                || err.contains("reload")
                || err.contains("missing"),
            "shell bind must fail stale without re-preparing, got: {err}"
        );
        let bind2 = classify_and_bind_chat_invoke(id, "get_weather", &params);
        assert_eq!(bind2.effect, CommandEffect::Mutating);
        invalidate_instance_capability(id);
    }

    #[test]
    fn shell_bind_without_token_fails_closed_not_fifo() {
        let id = "weather-forecast-shell-bind-v2";
        publish(id, CommandEffect::ReadOnly);
        let params = json!({"city": "Xiamen"});
        let _ = classify_and_bind_chat_invoke(id, "get_weather", &params);

        let argv = vec![
            "neomind".into(),
            "extension".into(),
            "invoke".into(),
            id.into(),
            "get_weather".into(),
            "--params".into(),
            r#"{"city":"Xiamen"}"#.into(),
        ];
        let err = bind_extension_invoke_prepared_token(argv).unwrap_err();
        assert!(
            err.contains("missing") || err.contains("unbound"),
            "must not FIFO-claim without token: {err}"
        );
        invalidate_instance_capability(id);
    }

    /// B5: argv `--prepared-operation-id` without host context is ignored and
    /// rejected as missing (host context is the sole source of tickets).
    #[test]
    fn shell_bind_ignores_argv_ticket_without_host_context() {
        let id = "weather-forecast-shell-bind-argv-only";
        publish(id, CommandEffect::ReadOnly);
        let params = json!({"city": "Xiamen"});
        let bind = classify_and_bind_chat_invoke(id, "get_weather", &params);
        let op = bind.prepared_operation_id.unwrap();

        let argv = vec![
            "neomind".into(),
            "extension".into(),
            "invoke".into(),
            id.into(),
            "get_weather".into(),
            "--params".into(),
            r#"{"city":"Xiamen"}"#.into(),
            "--prepared-operation-id".into(),
            op.clone(),
        ];
        let err = bind_extension_invoke_prepared_token(argv).unwrap_err();
        assert!(
            err.contains("PreparedInvoke missing"),
            "argv-only ticket must not be accepted: {err}"
        );
        assert!(
            peek_prepared_invoke(&op).is_some(),
            "ticket must remain untouched"
        );
        invalidate_instance_capability(id);
    }

    /// A RO classify → reload → B Mutating classify → A bind with A's id →
    /// stale. B's ticket stays unclaimed; "execute count" stays zero because
    /// bind fails before HTTP/extension dispatch.
    #[tokio::test]
    async fn shell_interleave_a_ro_reload_b_write_a_bind_stale_zero_exec() {
        static EXEC_COUNT: AtomicUsize = AtomicUsize::new(0);
        EXEC_COUNT.store(0, Ordering::SeqCst);

        let id = "weather-forecast-shell-bind-v3";
        publish(id, CommandEffect::ReadOnly);
        let params = json!({"city": "Xiamen"});
        let a = classify_and_bind_chat_invoke(id, "get_weather", &params);
        assert_eq!(a.effect, CommandEffect::ReadOnly);
        let a_id = a.prepared_operation_id.unwrap();

        publish(id, CommandEffect::Mutating);
        let b = classify_and_bind_chat_invoke(id, "get_weather", &params);
        assert_eq!(b.effect, CommandEffect::Mutating);
        let b_id = b.prepared_operation_id.unwrap();
        assert_ne!(a_id, b_id);

        let argv = vec![
            "neomind".into(),
            "extension".into(),
            "invoke".into(),
            id.into(),
            "get_weather".into(),
            "--params".into(),
            r#"{"city":"Xiamen"}"#.into(),
        ];
        let err = neomind_cli_ops::dispatch::with_extension_invoke_host_context(
            neomind_cli_ops::dispatch::ExtensionInvokeHostContext {
                prepared_operation_id: Some(a_id.clone()),
                structured_params: Some(params),
            },
            async { bind_extension_invoke_prepared_token(argv) },
        )
        .await
        .unwrap_err();
        assert!(
            err.contains("stale") || err.contains("missing") || err.contains("unbound"),
            "A must get stale, got: {err}"
        );
        // Bind failure means we never reached execute — count stays 0.
        assert_eq!(EXEC_COUNT.load(Ordering::SeqCst), 0);
        // B's ticket was not claimed/authorized by A's attempt.
        let b_peek = peek_prepared_invoke(&b_id).expect("B ticket intact");
        assert_eq!(b_peek.operation_id, b_id);
        assert_eq!(b_peek.contract.effect, CommandEffect::Mutating);
        invalidate_instance_capability(id);
    }
}

#[cfg(test)]
mod fabrication_guard_tests {
    use super::fabricated_tool_output;

    #[test]
    fn refuses_the_incident_shape() {
        // Shape from the 2026-10-15 incident: echo of a fake install-success
        // tool result (exit_code + command + stdout keys).
        let cmd = r#"echo "{\"exit_code\":0,\"stdout\":\"{\\\"success\\\": true}\",\"command\":\"neomind extension install face-recognition\"}""#;
        let real = fabricated_tool_output(cmd);
        assert!(real.is_some(), "must refuse the fabricated payload");
        assert!(
            real.as_deref().unwrap_or("").contains("extension"),
            "cure message should name the real command domain, got {real:?}"
        );
    }

    #[test]
    fn simple_echo_is_untouched() {
        assert_eq!(fabricated_tool_output("echo hello"), None);
        assert_eq!(
            fabricated_tool_output(r#"echo '{"success":true}'"#),
            None,
            "no exit_code+command signature"
        );
        assert_eq!(fabricated_tool_output("neomind extension list"), None);
        assert_eq!(fabricated_tool_output("printf '%s' hi"), None);
    }
}

#[cfg(test)]
mod p0_2_envelope_help_session_tests {
    use super::*;
    use crate::agent::task_continuity::outcome_is_pre_submission_rejection;
    use crate::toolkit::registry::ToolRegistryBuilder;
    use crate::toolkit::tool::{effective_success, Tool};
    use serde_json::json;

    fn enabled_shell_config() -> ShellConfig {
        ShellConfig {
            enabled: true,
            timeout_secs: 10,
            max_output_chars: 10000,
        }
    }

    #[test]
    fn in_process_domain_help_renders_subcommand_and_domain_from_clap() {
        let sub = ShellTool::in_process_domain_help(
            "dashboard",
            Some("update-component"),
            "retry with the exact one",
        )
        .expect("dashboard update-component help must render in-process");
        assert!(
            sub.contains("update-component")
                && sub.contains("--component-id")
                && sub.contains("--set"),
            "subcommand help must list exact flags: {sub}"
        );
        assert!(sub.lines().count() <= 30, "help must stay bounded: {sub}");

        // Unknown subcommand falls back to bare domain subcommand listing.
        let dom = ShellTool::in_process_domain_help(
            "dashboard",
            Some("nonexistent-sub"),
            "retry with the exact one",
        )
        .expect("unknown subcommand should fall back to domain help");
        assert!(
            dom.contains("add-components") && dom.contains("update-component"),
            "domain help must list real subcommands: {dom}"
        );

        // Unknown top-level domain returns None (no subprocess spawn).
        assert!(ShellTool::in_process_domain_help("no-such-domain", None, "note").is_none());
    }

    #[tokio::test]
    async fn parse_error_emits_usage_hint_in_separate_field_with_exit_code_2() {
        let tool = ShellTool::new(enabled_shell_config());
        let out = tool
            .execute(json!({
                "command": "neomind dashboard update-component dash-1 --bogus-flag 1"
            }))
            .await
            .unwrap();

        assert_eq!(
            out.data["exit_code"], 2,
            "clap parse error must keep exit_code=2"
        );
        assert_eq!(
            out.data["stdout"], "",
            "stdout must stay empty on parse error"
        );
        let stderr = out.data["stderr"].as_str().unwrap_or("");
        assert!(
            !stderr.contains("[Reference —"),
            "help must NOT be concatenated onto stderr: {stderr}"
        );
        let hint = out.data["usage_hint"]
            .as_str()
            .expect("parse error must attach in-process usage_hint");
        assert!(
            hint.contains("--component-id") && hint.contains("--set"),
            "usage_hint must come from in-process clap help: {hint}"
        );
        assert!(
            out.data["domain_index"]
                .as_str()
                .is_some_and(|s| s.contains("[neomind CLI domain index")),
            "first call in session should emit domain_index in its own field"
        );
        assert!(
            outcome_is_pre_submission_rejection("shell", &Ok(out)),
            "exit_code=2 parse failure must still classify as pre-submission rejection (Phase 1)"
        );
    }

    /// Spawn a tiny in-process HTTP server returning realistic `ApiResponse<T>`
    /// envelopes (including HTTP 200 embedded `"success": false` fake-successes).
    async fn spawn_mock_api_server() -> (String, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let first_line = req.lines().next().unwrap_or("");
                    let path = first_line.split_whitespace().nth(1).unwrap_or("");

                    let body = match path {
                        "/api/dashboards/dash-1" => json!({
                            "success": true,
                            "data": {
                                "id": "dash-1",
                                "name": "Ops",
                                "components": [
                                    {"id": "c1", "type": "metric", "position": {"x": 0, "y": 0, "w": 4, "h": 2}}
                                ]
                            },
                            "meta": {"timestamp": 1}
                        }),
                        "/api/rules/rule-1" => json!({
                            "success": true,
                            "data": {
                                "rule": {"id": "rule-1", "name": "HighTemp", "enabled": true},
                                "history": []
                            },
                            "meta": {"timestamp": 1}
                        }),
                        "/api/devices/dev-1/current" => json!({
                            "success": true,
                            "data": {
                                "device": {"id": "dev-1", "name": "Sensor"},
                                "metrics": {"temp": {"value": 23.5, "unit": "C", "timestamp": 1000}},
                                "commands": []
                            },
                            "meta": {"timestamp": 1}
                        }),
                        "/api/agents/agent-1" => json!({
                            "success": true,
                            "data": {"id": "agent-1", "name": "Monitor", "status": "active"},
                            "meta": {"timestamp": 1}
                        }),
                        "/api/brokers/b-1/test" => json!({
                            "success": true,
                            "data": {
                                "success": false,
                                "message": "Invalid broker configuration: empty host"
                            },
                            "meta": {"timestamp": 1}
                        }),
                        "/api/extensions/market/install" => json!({
                            "success": true,
                            "data": {
                                "success": false,
                                "extension_id": "bogus",
                                "downloaded": false,
                                "installed": false,
                                "error": "Extension not found: 404 Not Found"
                            },
                            "meta": {"timestamp": 1}
                        }),
                        _ => json!({
                            "success": true,
                            "data": {"path": path},
                            "meta": {"timestamp": 1}
                        }),
                    }
                    .to_string();

                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                });
            }
        });
        (format!("http://{addr}/api"), handle)
    }

    struct ApiBaseEnvGuard(Option<std::ffi::OsString>);
    impl ApiBaseEnvGuard {
        fn set(url: &str) -> Self {
            let prev = std::env::var_os("NEOMIND_API_BASE");
            std::env::set_var("NEOMIND_API_BASE", url);
            Self(prev)
        }
    }
    impl Drop for ApiBaseEnvGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(v) => std::env::set_var("NEOMIND_API_BASE", v),
                None => std::env::remove_var("NEOMIND_API_BASE"),
            }
        }
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn l2_single_layer_json_stdout_no_success_help_and_http_200_fake_success_blocked() {
        let (base_url, server) = spawn_mock_api_server().await;
        let _env = ApiBaseEnvGuard::set(&base_url);

        let tool = ShellTool::new(enabled_shell_config());

        // 1. First successful command (`dashboard get`):
        //    - stdout is 100% valid JSON (no plain-text `--help` or `DOMAIN_INDEX`)
        //    - single-layer CliResponse envelope (no nested `data.success` or `data.data`)
        //    - `domain_index` delivered once in its own field
        //    - `usage_hint` absent on success
        let dash = tool
            .execute(json!({"command": "neomind dashboard get dash-1"}))
            .await
            .unwrap();
        assert_eq!(dash.data["exit_code"], 0);
        assert!(effective_success("shell", &dash));
        assert!(
            dash.data.get("usage_hint").is_none(),
            "successful dashboard get must NOT attach 28-line --help: {}",
            dash.data
        );
        assert!(
            dash.data["domain_index"]
                .as_str()
                .is_some_and(|s| s.contains("[neomind CLI domain index")),
            "first session call must emit domain_index in separate field"
        );
        let stdout = dash.data["stdout"].as_str().unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(stdout).expect("stdout must be 100% valid JSON");
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["data"]["id"], "dash-1");
        assert!(
            parsed["data"].get("success").is_none() && parsed["data"].get("data").is_none(),
            "dashboard get must have single-layer envelope: {parsed}"
        );

        // 2. Subsequent successful calls (`rule get`, `device get`, `agent get`):
        //    all pure single-layer JSON, no repeated `domain_index`, no `usage_hint`.
        for (cmd, check_key) in [
            ("neomind rule get rule-1", "rule"),
            ("neomind device get dev-1", "device"),
            ("neomind agent get agent-1", "id"),
        ] {
            let out = tool.execute(json!({"command": cmd})).await.unwrap();
            assert_eq!(out.data["exit_code"], 0, "cmd `{cmd}` failed: {}", out.data);
            assert!(
                out.data.get("domain_index").is_none(),
                "domain_index must only fire once per session"
            );
            assert!(
                out.data.get("usage_hint").is_none(),
                "usage_hint must not fire on success"
            );
            let parsed: serde_json::Value =
                serde_json::from_str(out.data["stdout"].as_str().unwrap())
                    .expect("stdout must remain valid JSON");
            assert_eq!(parsed["success"], true);
            assert!(
                parsed["data"].get(check_key).is_some(),
                "expected `{check_key}` directly in single-layer data for `{cmd}`: {parsed}"
            );
            assert!(
                parsed["data"].get("success").is_none(),
                "nested `data.success` must not exist for `{cmd}`: {parsed}"
            );
        }

        // 3. HTTP 200 embedded `"success": false` fake-successes (`mqtt/brokers.rs`,
        //    `extensions/marketplace.rs`): must exit non-zero, effective_success=false,
        //    and NOT be misclassified as pre-submission `Rejected`.
        for (cmd, expected_err) in [
            (
                "neomind connector test b-1",
                "Invalid broker configuration: empty host",
            ),
            (
                "neomind extension market-install bogus",
                "Extension not found: 404 Not Found",
            ),
        ] {
            let out = tool.execute(json!({"command": cmd})).await.unwrap();
            assert_eq!(
                out.data["exit_code"], 1,
                "HTTP 200 embedded success:false must return non-zero exit_code for `{cmd}`: {}",
                out.data
            );
            assert!(
                !effective_success("shell", &out),
                "effective_success must be false for `{cmd}`"
            );
            assert!(
                !outcome_is_pre_submission_rejection("shell", &Ok(out.clone())),
                "server-side HTTP 200 business failure must NOT be misclassified as Rejected: {}",
                out.data
            );
            let stderr = out.data["stderr"].as_str().unwrap_or("");
            assert!(
                stderr.contains(expected_err),
                "stderr must carry real business error `{expected_err}`, got: {stderr}"
            );
        }

        server.abort();
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn l2_multi_session_domain_index_isolation_in_same_process() {
        let (base_url, server) = spawn_mock_api_server().await;
        let _env = ApiBaseEnvGuard::set(&base_url);

        // One shared registry (mirrors long-running server / SessionManager).
        let shared_registry = ToolRegistryBuilder::new()
            .with_shell_tool(Some(enabled_shell_config()))
            .build();

        let session_a = shared_registry.for_chat_session("session-a");
        let session_b = shared_registry.for_chat_session("session-b");

        let shell_a = session_a.get("shell").expect("shell in session A");
        let shell_b = session_b.get("shell").expect("shell in session B");

        // Session A: call 1 gets domain_index, call 2 does not.
        let a1 = shell_a
            .execute(json!({"command": "neomind dashboard get dash-1"}))
            .await
            .unwrap();
        assert!(
            a1.data["domain_index"].is_string(),
            "Session A first call must receive domain_index"
        );
        let a2 = shell_a
            .execute(json!({"command": "neomind rule get rule-1"}))
            .await
            .unwrap();
        assert!(
            a2.data.get("domain_index").is_none(),
            "Session A second call must not repeat domain_index"
        );

        // Session B (same process, started after Session A): call 1 MUST STILL
        // receive domain_index — proving no process-global static leakage.
        let b1 = shell_b
            .execute(json!({"command": "neomind dashboard get dash-1"}))
            .await
            .unwrap();
        assert!(
            b1.data["domain_index"].is_string(),
            "Session B first call must receive domain_index independently of Session A"
        );
        let b2 = shell_b
            .execute(json!({"command": "neomind rule get rule-1"}))
            .await
            .unwrap();
        assert!(
            b2.data.get("domain_index").is_none(),
            "Session B second call must not repeat domain_index"
        );

        // Clearing history resets Session A's injection state without affecting Session B.
        session_a.reset_session_state();
        let a3 = shell_a
            .execute(json!({"command": "neomind agent get agent-1"}))
            .await
            .unwrap();
        assert!(
            a3.data["domain_index"].is_string(),
            "Session A after reset_session_state must receive domain_index again"
        );
        let b3 = shell_b
            .execute(json!({"command": "neomind agent get agent-1"}))
            .await
            .unwrap();
        assert!(
            b3.data.get("domain_index").is_none(),
            "Resetting Session A must not affect Session B"
        );

        server.abort();
    }

    /// P2-3 Anti-Drift Scanner: every `neomind <domain> <subcommand>` referenced
    /// in `DOMAIN_INDEX` and in ALL builtin skill guides (`skills/builtins/*.md`,
    /// both command examples and YAML `tool_target`) must resolve to a real
    /// clap subcommand in `neomind_cli_ops::dispatch::commands::Args`.
    #[test]
    fn anti_drift_scanner_builtin_skills_and_domain_index_match_clap_tree() {
        use clap::CommandFactory;
        use std::collections::{BTreeMap, BTreeSet};

        let root = neomind_cli_ops::dispatch::commands::Args::command();
        // Map domain name -> set of valid subcommand names + aliases.
        let mut domain_subcommands: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for sub in root.get_subcommands() {
            if !sub.has_subcommands() {
                continue;
            }
            let mut names = BTreeSet::new();
            for child in sub.get_subcommands() {
                names.insert(child.get_name().to_string());
                for alias in child.get_all_aliases() {
                    names.insert(alias.to_string());
                }
            }
            domain_subcommands.insert(sub.get_name().to_string(), names);
        }

        let mut failures: Vec<String> = Vec::new();

        // 1) Scan `DOMAIN_INDEX` bullet lines (`- <domain>: verb1 verb2 ...`).
        for line in DOMAIN_INDEX.lines() {
            let Some(rest) = line.trim().strip_prefix("- ") else {
                continue;
            };
            let Some((domain, body)) = rest.split_once(':') else {
                continue;
            };
            let domain = domain.trim();
            let Some(valid) = domain_subcommands.get(domain) else {
                failures.push(format!("DOMAIN_INDEX: unknown domain `{domain}`"));
                continue;
            };
            // Stop before parenthetical prose notes.
            let verbs_part = body.split('(').next().unwrap_or(body);
            for tok in verbs_part.split_whitespace() {
                // Skip placeholder args like `<ID>`, `<CMD>`, `<active|paused>`.
                if tok.starts_with('<') || tok.starts_with('[') || tok.starts_with('-') {
                    continue;
                }
                if !valid.contains(tok) {
                    failures.push(format!(
                        "DOMAIN_INDEX: `neomind {domain} {tok}` does not exist in Args::command()"
                    ));
                }
            }
        }

        // 2) Scan every builtin skill `.md` file under `src/skills/builtins/`.
        //    Inspects YAML `tool_target` entries, fenced code blocks, and inline
        //    backtick code spans so prose words after `` `neomind settings` ``
        //    are never misread as subcommands.
        let builtins_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/skills/builtins");
        let mut md_paths: Vec<std::path::PathBuf> = std::fs::read_dir(&builtins_dir)
            .expect("read src/skills/builtins")
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
            .collect();
        md_paths.sort();
        assert!(
            !md_paths.is_empty(),
            "expected builtin skill markdown files in {}",
            builtins_dir.display()
        );

        let is_cli_ident = |s: &str| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && s.as_bytes()[0].is_ascii_lowercase()
        };

        let check_code_snippet = |snippet: &str,
                                  file: &str,
                                  line_no: usize,
                                  domain_subcommands: &BTreeMap<String, BTreeSet<String>>,
                                  failures: &mut Vec<String>| {
            let mut search_from = 0usize;
            while let Some(rel) = snippet[search_from..].find("neomind ") {
                let pos = search_from + rel;
                search_from = pos + "neomind ".len();
                if pos > 0 {
                    let prev = snippet.as_bytes()[pos - 1];
                    if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'-' {
                        continue;
                    }
                }
                let after = &snippet[pos + "neomind ".len()..];
                let mut tokens = after.split_whitespace();
                let Some(raw_dom) = tokens.next() else {
                    continue;
                };
                let dom = raw_dom.trim_matches(|c: char| {
                    matches!(c, '"' | '\'' | '(' | ')' | ',' | '.' | ':' | ';' | '|')
                });
                let Some(valid) = domain_subcommands.get(dom) else {
                    continue;
                };
                let Some(raw_sub) = tokens.next() else {
                    continue;
                };
                let sub = raw_sub.trim_matches(|c: char| {
                    matches!(c, '"' | '\'' | '(' | ')' | ',' | '.' | ':' | ';' | '|')
                });
                if !is_cli_ident(sub) {
                    continue;
                }
                if !valid.contains(sub) {
                    failures.push(format!(
                        "{file}:{line_no} `neomind {dom} {sub}` does not exist in Args::command()"
                    ));
                }
            }
        };

        for path in md_paths {
            let file = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            let content = std::fs::read_to_string(&path).expect("read builtin skill");

            let mut in_frontmatter = false;
            let mut in_code_fence = false;
            let mut current_tool: Option<String> = None;
            for (idx, raw_line) in content.lines().enumerate() {
                let line_no = idx + 1;
                let trimmed = raw_line.trim();
                if idx == 0 && trimmed == "---" {
                    in_frontmatter = true;
                    continue;
                }
                if in_frontmatter {
                    if trimmed == "---" {
                        in_frontmatter = false;
                        continue;
                    }
                    if let Some(t) = trimmed.strip_prefix("- tool:") {
                        current_tool = Some(t.trim().to_string());
                    } else if let Some(rest) = trimmed.strip_prefix("actions:") {
                        if let Some(domain) = current_tool.as_deref() {
                            if let Some(valid) = domain_subcommands.get(domain) {
                                let list =
                                    rest.trim().trim_start_matches('[').trim_end_matches(']');
                                for act in list.split(',').map(str::trim).filter(|s| !s.is_empty())
                                {
                                    if !valid.contains(act) {
                                        failures.push(format!(
                                            "{file}:{line_no} tool_target `{domain}` action `{act}` does not exist in Args::command()"
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    continue;
                }

                if trimmed.starts_with("```") {
                    in_code_fence = !in_code_fence;
                    continue;
                }

                if in_code_fence {
                    let code_part = raw_line.split('#').next().unwrap_or(raw_line);
                    check_code_snippet(
                        code_part,
                        file,
                        line_no,
                        &domain_subcommands,
                        &mut failures,
                    );
                    continue;
                }

                // Skip explicit negative examples documenting non-existent commands.
                if raw_line.contains("do NOT exist") || raw_line.contains("does NOT exist") {
                    continue;
                }

                // Scan inline backtick code spans: `neomind <domain> <subcommand> ...`
                let mut rest = raw_line;
                while let Some(open) = rest.find('`') {
                    let after_open = &rest[open + 1..];
                    let Some(close) = after_open.find('`') else {
                        break;
                    };
                    let span = &after_open[..close];
                    check_code_snippet(span, file, line_no, &domain_subcommands, &mut failures);
                    rest = &after_open[close + 1..];
                }
            }
        }

        assert!(
            failures.is_empty(),
            "Anti-drift scanner found {} stale CLI reference(s):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn shell_command_param_pipe_description_matches_capability_index() {
        let params = ShellTool::new(ShellConfig::default()).parameters();
        let cmd_desc = params["properties"]["command"]["description"]
            .as_str()
            .unwrap();
        assert!(
            cmd_desc.contains("built-in filter flags") && cmd_desc.contains("native host commands"),
            "shell command param description must unify pipe rules with capability_index: {cmd_desc}"
        );
    }
}
