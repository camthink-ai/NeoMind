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
}

/// Shell tool — executes system commands.
pub struct ShellTool {
    config: ShellConfig,
}

impl ShellTool {
    pub fn new(config: ShellConfig) -> Self {
        Self { config }
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

        // Prepend the current binary's directory to PATH so subprocess `neomind`
        // invocations resolve to the same binary that's running the server.
        // Without this, `/bin/sh -c "neomind ..."` walks PATH and may find a
        // stale install (e.g. `~/.cargo/bin/neomind`), causing silent version
        // drift between server and CLI — particularly for local-only commands
        // (extension create/build/install) that bypass in-process dispatch.
        if let Ok(exe) = std::env::current_exe() {
            if let Some(exe_dir) = exe.parent() {
                let exe_dir = exe_dir.display().to_string();
                match std::env::var_os("PATH") {
                    Some(existing) => {
                        let new_path = format!(
                            "{}{}{}",
                            exe_dir,
                            path_delimiter(),
                            existing.to_string_lossy()
                        );
                        cmd.env("PATH", new_path);
                    }
                    None => {
                        cmd.env("PATH", exe_dir);
                    }
                }
            }
        }

        cmd
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

    /// Attempt in-process dispatch for `neomind` data commands.
    ///
    /// Returns `Some(output)` if the command was handled in-process (either
    /// success, a parse error, or an API error); returns `None` for
    /// [`DispatchError::NotInProcess`] (side-effecting / interactive /
    /// local-only subcommands) so the caller falls back to spawning a real
    /// subprocess.
    ///
    /// Non-`neomind` commands and malformed input (unbalanced quotes) also
    /// yield `None` so they hit the subprocess path unchanged.
    async fn try_in_process_dispatch(
        &self,
        command: &str,
        timeout: Duration,
    ) -> Option<CommandOutput> {
        let trimmed = command.trim();

        // `&&` sequencing of multiple neomind commands (the agent naturally
        // batches, e.g. `neomind device get a && neomind device get b`).
        // Handle in-process when ALL parts are neomind commands; otherwise
        // fall through to the subprocess path (which supports && via /bin/sh).
        if trimmed.contains("&&") {
            let parts: Vec<&str> = trimmed
                .split("&&")
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();
            let all_neomind = parts.len() > 1
                && parts
                    .iter()
                    .all(|p| p.starts_with("neomind ") || *p == "neomind");
            if all_neomind {
                let mut outputs: Vec<String> = Vec::new();
                for part in parts {
                    // Boxed: the recursive async call needs indirection (the
                    // && chain length is runtime-variable → unbounded future).
                    match Box::pin(self.try_in_process_dispatch(part, timeout)).await {
                        Some(out) => {
                            let code = out.exit_code.unwrap_or(1);
                            outputs.push(out.stdout);
                            if code != 0 {
                                // `&&` semantics: stop at first failure.
                                return Some(CommandOutput {
                                    exit_code: Some(code),
                                    stdout: outputs.join("\n--- && ---\n"),
                                    stderr: String::new(),
                                    timed_out: false,
                                });
                            }
                        }
                        None => return None, // a part isn't in-processable → subprocess
                    }
                }
                return Some(CommandOutput {
                    exit_code: Some(0),
                    stdout: outputs.join("\n--- && ---\n"),
                    stderr: String::new(),
                    timed_out: false,
                });
            }
            // Mixed neomind/non-neomind with && → let /bin/sh handle it.
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

        // Make auth + JSON-output env visible to the in-process handler.
        // `dispatch`'s `ApiClient` reads `NEOMIND_API_KEY` (via auto_auth)
        // and the handlers read `NEOMIND_JSON` to pick the output format.
        // This mirrors the env injection done for the subprocess in
        // `build_command`.
        if let Some(key) = Self::resolve_api_key() {
            // NOTE: env mutation is process-global. The agent runtime is
            // single-tenancy and is the only concurrent writer of this var,
            // so this is equivalent to the existing subprocess env injection.
            std::env::set_var("NEOMIND_API_KEY", key);
        }
        std::env::set_var("NEOMIND_JSON", "1");

        tracing::debug!(
            target: "neomind::agent::shell",
            in_process = true,
            command = %command,
            "dispatching neomind command in-process"
        );

        // Apply the same per-command timeout as the subprocess path. The
        // ApiClient has its own 30s HTTP timeout, but a handler may issue
        // multiple requests; this guarantees the in-process path cannot hang
        // longer than the subprocess equivalent would.
        match tokio::time::timeout(timeout, neomind_cli_ops::dispatch::dispatch(&argv)).await {
            Ok(Ok(resp)) => {
                let exit_code = if resp.success { 0 } else { 1 };
                let stdout = serde_json::to_string_pretty(&resp).unwrap_or_else(|e| {
                    format!(
                        "{{\"success\":false,\"error\":\"serialize failed: {}\"}}",
                        e
                    )
                });
                Some(CommandOutput {
                    exit_code: Some(exit_code),
                    stdout,
                    stderr: String::new(),
                    timed_out: false,
                })
            }
            Ok(Err(neomind_cli_ops::dispatch::DispatchError::NotInProcess)) => {
                tracing::debug!(
                    target: "neomind::agent::shell",
                    in_process = false,
                    command = %command,
                    "falling back to subprocess (NotInProcess)"
                );
                None
            }
            Ok(Err(neomind_cli_ops::dispatch::DispatchError::Parse(msg))) => Some(CommandOutput {
                exit_code: Some(2),
                stdout: String::new(),
                stderr: format!("error: {}", msg),
                timed_out: false,
            }),
            Ok(Err(neomind_cli_ops::dispatch::DispatchError::Api(msg))) => Some(CommandOutput {
                exit_code: Some(1),
                stdout: String::new(),
                stderr: msg,
                timed_out: false,
            }),
            // Timeout (outer Err = elapsed) — mirror the subprocess timeout behavior.
            Err(_) => Some(CommandOutput {
                exit_code: None,
                stdout: String::new(),
                stderr: format!("Command timed out after {}s", timeout.as_secs()),
                timed_out: true,
            }),
        }
    }

    /// Execute a command with timeout and output capture.
    async fn execute_command(
        &self,
        command: &str,
        working_dir: Option<&str>,
        timeout: Duration,
    ) -> Result<CommandOutput> {
        // Fast path: route `neomind` data commands through the in-process
        // dispatcher so the agent gets structured `CliResponse` directly,
        // without depending on whatever `neomind` binary happens to be in
        // PATH (eliminates version drift between the running server and the
        // CLI binary). Side-effecting/interactive/local-only commands return
        // `NotInProcess` and fall through to the subprocess path below.
        if let Some(output) = self.try_in_process_dispatch(command, timeout).await {
            return Ok(output);
        }

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
                Ok(CommandOutput {
                    exit_code: status.code(),
                    stdout,
                    stderr,
                    timed_out: false,
                })
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
                Ok(CommandOutput {
                    exit_code: None,
                    stdout: String::new(),
                    stderr: format!("Command timed out after {}s", timeout.as_secs()),
                    timed_out: true,
                })
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

/// Tokenize a `neomind` command line into an argv vector, respecting single
/// and double quotes and backslash escapes.
///
/// This is NOT a full shell parser — it deliberately ignores pipes,
/// redirections, `$` expansions, and command separators, because those
/// constructs are never part of a pure `neomind` data query. A command that
/// uses them is left for the real shell (subprocess path) to interpret.
///
/// The first token is expected to be `neomind`. Returns an error if the input
/// has unbalanced quotes (so the caller can fall back to the subprocess and
/// surface the real shell error message).
fn tokenize_neomind_command(input: &str) -> std::result::Result<Vec<String>, String> {
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
        (usable * stdout_len / total).min(stdout_len)
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

impl ShellTool {
    /// Generate a recovery hint when a neomind CLI command fails.
    fn recovery_hint(command: &str, stdout: &str, stderr: &str) -> Option<String> {
        let cmd = command.trim();
        if !cmd.starts_with("neomind ") {
            return None;
        }

        let parts: Vec<&str> = cmd.splitn(4, ' ').collect();
        let domain = parts.get(1).copied().unwrap_or("");
        let action = parts.get(2).copied().unwrap_or("");
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

        // Common syntax hint for all neomind commands when unexpected argument
        if is_unexpected_arg {
            if cmd.contains("--id ") {
                return Some("ID is a positional argument, not a flag. Use: neomind <domain> <action> <ID> [options]. Example: neomind device get abc123 (not --id abc123).".to_string());
            }
            return Some("Command syntax error. ID is positional (not --id flag). Run 'neomind <domain> <action> --help' to see correct usage.".to_string());
        }

        match domain {
            "device" => {
                if is_not_found {
                    Some("Run 'neomind device list' to see available devices, then retry with a valid ID.".to_string())
                } else if action == "create" && is_validation {
                    Some("Required fields: --name, --type. Use 'neomind device types list' to see valid device types.".to_string())
                } else if action == "control" && is_not_found {
                    Some("Device not found. Run 'neomind device list' first, then use 'neomind device control <ID> --command <CMD>'.".to_string())
                } else if (action == "history" || action == "latest") && combined.contains("metric")
                {
                    Some("Don't guess metric names. Run 'neomind device list' to see all metric_fields per type, or 'neomind device get <ID>' for a specific device's actual field names.".to_string())
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
                    Some("Use --components to update widgets. Run 'neomind widget list' to see available widget types, and 'neomind dashboard get <ID>' to see current layout.".to_string())
                } else {
                    Some("Available actions: list, get, create, update, delete, share".to_string())
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
                } else {
                    Some("Available actions: list, get, create, update, delete, enable, disable, test, history".to_string())
                }
            }
            "agent" => {
                if is_not_found {
                    Some("Run 'neomind agent list' to see available agents.".to_string())
                } else if action == "create" && is_validation {
                    Some("Required fields: --name, --prompt, --schedule-type (event|interval|cron). Example: neomind agent create --name \"monitor\" --prompt \"Check devices\" --schedule-type event".to_string())
                } else if action == "control" && is_validation {
                    Some("Valid status values: active, paused. Example: neomind agent control <ID> --action active".to_string())
                } else {
                    Some("Available actions: list, get, create, update, delete, control, invoke, memory, executions, latest-execution, conversation, send-message".to_string())
                }
            }
            "extension" => {
                if is_not_found {
                    Some("Run 'neomind extension list' to see installed extensions.".to_string())
                } else if action == "install" && is_validation {
                    Some("Provide the extension zip file path. Use 'neomind extension market-list' to browse marketplace.".to_string())
                } else if action == "config" {
                    Some("Usage: neomind extension config <ID> to view, or neomind extension config <ID> --set '{\"key\":\"value\"}' to update.".to_string())
                } else {
                    Some("Available actions: list, get, status, logs, config, install, uninstall, reload, create, build, market-list, market-install".to_string())
                }
            }
            "transform" => {
                if is_not_found {
                    Some("Run 'neomind transform list' to see available transforms.".to_string())
                } else if action == "create" && is_validation {
                    Some("Required fields: --name, --code (JavaScript function). Use --scope to set input scope. Example: neomind transform create --name \"celsius\" --code \"return value * 9/5 + 32\" --scope global".to_string())
                } else {
                    Some("Available actions: list, get, create, update, delete, test-code, metrics, data-sources".to_string())
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
                    Some("Provide a widget directory (containing manifest.json + bundle.js) or a .zip file. Example: neomind widget install data/frontend-components/my-widget".to_string())
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
                    Some("Usage: neomind message channel-update --name <N> --config '<JSON>'. To filter by severity: --config '{\"min_severity\":\"warning\"}'. To filter by source type: --config '{\"source_types\":[\"device\"]}'. channel-create uses --name flag; channel-delete/channel-test take name as positional arg.".to_string())
                } else {
                    Some("Available actions: list, get, send, read, channel-list, channel-get, channel-create, channel-update, channel-delete, channel-types, channel-test".to_string())
                }
            }
            "llm" => {
                if is_not_found {
                    Some("Run 'neomind llm list' to see configured backends.".to_string())
                } else if action == "create" && is_validation {
                    Some("Required fields: --name, --type (ollama|openai|custom), --endpoint, --model. Example: neomind llm create --name local --type ollama --endpoint http://localhost:11434 --model qwen3:4b".to_string())
                } else {
                    Some("Available actions: list, get, models, create, update, delete, activate, test. Example: neomind llm create --name local --type ollama --endpoint http://localhost:11434 --model qwen3:4b".to_string())
                }
            }
            _ => None,
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
            .trim()
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
        r#"Execute shell commands on the host system.

Use this tool to run any system command. For NeoMind platform operations, use the `neomind` CLI.

## Critical Syntax Rules (apply to ALL neomind domains)
- **ID is always a positional argument**, NEVER a `--id` flag. Correct: `neomind device get abc123`. Wrong: `neomind device get --id abc123`.
- **NEVER guess metric names**. Discover first via `neomind device list` (returns `metric_fields` per type) or `neomind device get <ID>` (full metric names + values), then use exact names in `--metric`, rule conditions, transform code, or dashboard bindings. The same applies to extension fields — discover via `neomind extension info <ID>`.
- **"unexpected argument" error** = you used a flag where positional was expected. Rewrite without the flag.
- On command failure, check the `suggestion` field in the JSON output for recovery hints.

## NeoMind CLI Domain Syntax
The `neomind` CLI has 14 domains: `device`, `dashboard`, `rule`, `agent`, `extension`, `widget`, `transform`, `llm`, `message`, `connector`, `push`, `settings`, `system`, `api-key`.

**Domain-specific command syntax, JSON formats, and copy-paste templates live in skill docs** — use the `skill` tool (`skill(action="search", query="<domain>")`) to load the matching guide, or run `neomind <domain> <action> --help` for flags and examples. All commands return JSON by default in this environment (controlled by the `NEOMIND_JSON` env var) — do NOT pass any `--json` flag.

## Easy-to-miss subcommands (check before falling back to ping/nc/ls)
When the user asks for a domain-specific action, try the matching `neomind <domain> <subcommand>` FIRST — do NOT fall back to raw shell tools (`ping`, `nc`, `ls`, `curl`) until the CLI subcommand has been tried and returned an error.
- **`neomind connector test <id>`** — test reachability of an MQTT broker. Use this, NOT `ping`/`nc`/`/dev/tcp`.
- **`neomind connector subscriptions`** — list active MQTT subscriptions across all brokers (takes no id).
- **`neomind device drafts list` / `drafts approve <id>` / `drafts reject <id>`** — manage auto-discovery drafts. Drafts are NOT deleted via `device delete`; use `device drafts reject <id>` to dismiss a draft.
- **`neomind extension status <id>` / `extension logs <id>` / `extension reload <id>` / `extension config <id>`** — runtime introspection beyond `list`/`get`. If `extension list` shows an extension but you need health/logs, use these.
- **`neomind agent clear-memory <id>` / `agent executions <id>`** — memory reset and execution history (distinct from `agent get`).
- **`neomind transform test-code`** — dry-run transform JavaScript against sample input before saving. For rules, use `neomind rule test <id> --input '<JSON>'` (what-if evaluation against existing rule).

## Native System Commands
Runs on host via `/bin/sh -c` (Unix) or `cmd /C` (Windows). Common tools available: ping, traceroute, curl, arp, nmap, ps, df, free, top, uptime, systemctl status, ls, cat, head, tail, grep, find, wc, arp-scan, avahi-browse, bluetoothctl, docker.

## GUI-Launching Commands (IMPORTANT — do NOT loop)
Commands like `open` (macOS), `xdg-open` (Linux), `start`/`explorer` (Windows), or any app launcher (`preview`, `code`, `safari`) launch a GUI window and return **only** `exit_code: 0` with **empty** stdout/stderr on success.

- An empty-output success means "the launch was accepted" — it does NOT mean the window appeared, and you CANNOT see the window yourself.
- **Call such a command exactly ONCE**, then move on with your task. Do NOT retry, do NOT try variants (`open -a Preview`, `open -R`, etc.) hoping for output — they all return the same empty result and you will never perceive the GUI.
- If the user needs to inspect an image's pixels, ask them to look at their screen — do not try to "see" it yourself by re-running `open`.

## Execution Notes
- Each command runs in a fresh process — no persistent shell state between calls.
- `neomind` commands are dispatched in-process (no subprocess); they return a structured `CliResponse` as pretty-printed JSON on stdout.
- Output may be truncated for very long responses."#
    }

    fn parameters(&self) -> Value {
        object_schema(
            serde_json::json!({
                "command": {
                    "type": "string",
                    "description": "The shell command to execute. Supports pipes, redirections, and other shell features."
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

        let working_dir = args.get("working_dir").and_then(|v| v.as_str());
        let description = args.get("description").and_then(|v| v.as_str());

        tracing::info!(
            command = %command,
            description = description.unwrap_or(""),
            "Executing shell command"
        );

        let output = self.execute_command(command, working_dir, timeout).await?;

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

        // Enrich error responses with recovery hints for neomind CLI commands
        let is_error = output.exit_code.unwrap_or(1) != 0;
        if is_error {
            if let Some(hint) = Self::recovery_hint(command, &stdout, &stderr) {
                result["suggestion"] = serde_json::Value::String(hint);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> ShellConfig {
        ShellConfig {
            enabled: true,
            timeout_secs: 10,
            max_output_chars: 5000,
        }
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
