//! Judgment-notify gate: block `neomind message send` until a valid verdict
//! says alert criteria are met.
//!
//! For agents with `notify.on = Judgment`, the machinery never routes a
//! notification itself — the agent speaks via `shell` → `neomind message send`
//! (or a hallucinated CLI-domain tool rewritten to that shell command).
//! Keyword-scanning the *final* reply after a successful send cannot unsend,
//! so this module classifies the turn's judgment and must run **before** the
//! first real delivery. Ordinary (`Always` / `Failure`) notify agents keep
//! their previous contract unchanged.
//!
//! ## Auth source (last-line Verdict protocol — not substring / lexicon)
//! Authorization reads **only the last non-empty line** of the latest
//! assistant message, with the tool-call payload removed first (every
//! backend appends the native tool-call array to the response text, so the
//! raw last line is the serialized call, never the verdict). That line must
//! **exactly** match one of:
//! `Verdict: criteria met` / `Verdict: criteria not met` /
//! `Verdict: insufficient evidence` (case-insensitive; optional trailing
//! period). Backtick-wrapped examples, bare labels, mid-message protocol
//! lines, and explanatory prose that mentions a label do **not** grant
//! delivery. Missing/invalid protocol → deny. Prior-event conclusions must
//! not interfere (latest assistant message only).

use neomind_core::message::{Message, MessageRole};
use neomind_storage::{AiAgent, NotifyOn};
use serde_json::Value;

/// Inventory of `notify.on=Judgment` delivery entry points (test-facing).
///
/// Tool-loop paths (1–2) must share [`judgment_deny_for_tool_call`] in
/// `tool_loop.rs` after CLI-domain → shell rewrite. Machinery paths (3–4)
/// silence Judgment without the semantic gate (no agent-authored send).
/// Kept under `cfg(test)` so `-D warnings` lib builds stay clean while the
/// inventory remains the review checklist for shared-gate coverage.
#[cfg(test)]
pub(crate) const JUDGMENT_NOTIFY_ENTRY_POINTS: &[&str] = &[
    // (1) Direct shell: `neomind message send` / bare `message send`
    "tool_loop:shell.message_send",
    // (2) Hallucinated CLI-domain tools rewritten to shell by build_cli_command
    //     (`message` / aliases → `neomind message send …`)
    "tool_loop:rewritten_cli_domain.message_send",
    // (3) Routed machinery — NotifyOn::Judgment ⇒ should_send=false
    "command_executor:dispatch_agent_notifications",
    // (4) Legacy keyword / condition_met alerts — only when notify is None;
    //     Judgment agents have notify set, so this path must stay closed
    "command_executor:send_alert_for_decision(legacy,notify=None_only)",
];

/// Outcome of classifying the agent's judgment for this event/turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JudgmentVerdict {
    /// Alert criteria are met — a notification may be delivered.
    CriteriaMet,
    /// Criteria clearly not met — patrol may complete; no alert.
    CriteriaNotMet,
    /// Cannot decide (dark / blur / missing evidence) — no alert.
    InsufficientEvidence,
    /// No recognizable finished judgment yet — refuse send rather than guess.
    Unknown,
}

/// True when this agent relies on tool-driven notify (`on = judgment`).
pub(crate) fn agent_uses_judgment_notify(agent: &AiAgent) -> bool {
    matches!(
        agent.notify.as_ref().map(|n| &n.on),
        Some(NotifyOn::Judgment)
    )
}

/// Outbound web tools a judgment-notify agent does not get by default.
///
/// Its verdict comes from the input attached to the run. A web search only
/// gives a small model somewhere to hunt for what it believes is missing,
/// and it leaves the site with the query — T4's three camera agents ran 233
/// searches that carried their device IDs and the site name.
pub(crate) const JUDGMENT_DEFAULT_OFF_TOOLS: &[&str] = &["web_search", "web_fetch"];

/// True for a web tool on a judgment-notify agent whose allowlist does not
/// name it. An explicit `allowed_tools` entry is the operator's word — it wins.
pub(crate) fn judgment_tool_off_by_default(agent: &AiAgent, tool: &str) -> bool {
    agent_uses_judgment_notify(agent)
        && JUDGMENT_DEFAULT_OFF_TOOLS.contains(&tool)
        && !agent
            .tool_config
            .as_ref()
            .is_some_and(|config| config.allowed_tools.iter().any(|t| t == tool))
}

/// Shells that run a command line given as text.
const PASS_THROUGH_SHELLS: &[&str] = &["sh", "bash", "zsh", "dash", "ksh"];

/// Programs that run whatever their argument says. Judged on the raw text.
const INTERPRETERS: &[&str] = &[
    "python",
    "python3",
    "perl",
    "ruby",
    "node",
    "php",
    "awk",
    "gawk",
    "xargs",
    "find",
    "osascript",
    "cmd",
    "powershell",
    "pwsh",
];

/// Commands that pass the rest of their argv through. A bare `message send`
/// behind one of these is still the command.
const WRAPPERS: &[&str] = &["sudo", "env", "nohup", "time", "command", "exec"];

/// How far a command line may sit inside another (`sh -c "…"`, `$(…)`).
const MAX_COMMAND_NESTING: usize = 4;

/// True when a shell command runs `neomind message send` anywhere in it.
///
/// Judged by argv, one command of the line at a time — the argv the shell
/// tool dispatches with, so whitespace runs, quoting and escapes collapse to
/// the same tokens. A quoted argument is a single token: parameter text that
/// merely quotes the words "message send" is text, not a send.
pub(crate) fn is_message_send_command(command: &str) -> bool {
    command_line_runs_message_send(command, 0)
}

fn command_line_runs_message_send(line: &str, depth: usize) -> bool {
    if depth > MAX_COMMAND_NESTING {
        return message_send_words_in(line, true);
    }
    shell_commands(line)
        .iter()
        .any(|command| command_runs_message_send(command, depth))
}

/// One command of a line, judged by its argv. Anything that is not a plain
/// argv — a redirection, an unbalanced quote — is judged on its words:
/// blocking too much costs the model one retry, letting a send through
/// costs the operator an alert they did not want.
fn command_runs_message_send(command: &str, depth: usize) -> bool {
    let command = command.trim();
    if command.is_empty() {
        return false;
    }
    let argv: Vec<String> = match crate::toolkit::shell::tokenize_neomind_command(command) {
        Ok(argv) if !argv.is_empty() => argv,
        _ => return message_send_words_in(command, false),
    };
    if argv_runs_message_send(&argv) {
        return true;
    }
    let program = program_name(&argv[0]);
    if PASS_THROUGH_SHELLS.contains(&program) {
        // `sh -c '<line>'`: the operand is a command line of its own.
        return match shell_c_operand(&argv) {
            Some(operand) => command_line_runs_message_send(operand, depth + 1),
            None => false,
        };
    }
    if program == "eval" {
        return command_line_runs_message_send(&argv[1..].join(" "), depth + 1);
    }
    // It runs whatever its argument says — quoted or not.
    INTERPRETERS.contains(&program) && message_send_words_in(command, true)
}

/// `neomind message send` as three adjacent argv tokens, wherever it sits
/// (`sudo neomind …`, `NEOMIND_JSON=1 neomind …`, `timeout 30 neomind …`),
/// or a bare `message send` — the hallucinated CLI-domain form — at the
/// start, behind a wrapper, or behind a program token that expands to
/// neomind (`"$(which neomind)"`).
fn argv_runs_message_send(argv: &[String]) -> bool {
    let is_neomind = |token: &str| program_name(token) == "neomind";
    argv.windows(3)
        .any(|w| is_neomind(&w[0]) && w[1] == "message" && w[2] == "send")
        || argv.windows(2).enumerate().any(|(i, w)| {
            w[0] == "message"
                && w[1] == "send"
                && (i == 0
                    || WRAPPERS.contains(&program_name(&argv[i - 1]))
                    || is_env_assignment(&argv[i - 1])
                    || i == 1 && argv[0].to_ascii_lowercase().contains("neomind"))
        })
}

/// The command string of `sh -c '<line>'` (option clusters such as `-lc`
/// count). `None` for `sh script.sh`: the operand is a file, not a line.
fn shell_c_operand(argv: &[String]) -> Option<&str> {
    let mut args = argv.iter().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--" {
            return args.next().map(String::as_str);
        }
        if arg.starts_with('-') && arg.len() > 1 {
            if arg.contains('c') {
                return args.next().map(String::as_str);
            }
            continue;
        }
        return None;
    }
    None
}

/// The program a token names: `sudo`'s `(sudo` and a subshell's `(neomind`
/// lose their brackets, a full path keeps only its last component.
fn program_name(token: &str) -> &str {
    let token = token
        .trim_start_matches(['(', '{'])
        .trim_end_matches([')', '}', ';', ',']);
    token.rsplit('/').next().unwrap_or(token)
}

/// `NAME=value` — an environment assignment ahead of the command.
fn is_env_assignment(token: &str) -> bool {
    token.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// The commands a shell would run for `line`: the parts between unquoted
/// `;`, `&&`, `||`, `|`, `&` and newlines, then what sits in `$(…)` and
/// backticks, which run as lines of their own.
fn shell_commands(line: &str) -> Vec<&str> {
    let mut commands = Vec::new();
    let mut nested = Vec::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut start = 0usize;
    let mut prev = '\0';
    let mut chars = line.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' if !in_single => {
                chars.next();
            }
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '`' if !in_single => {
                let from = i + 1;
                let to = skip_past(&mut chars, line.len(), |c| c == '`');
                nested.push(&line[from..to]);
            }
            '$' if !in_single && chars.peek().is_some_and(|(_, n)| *n == '(') => {
                chars.next();
                let from = i + 2;
                let to = skip_parens(&mut chars, line.len());
                nested.push(&line[from..to]);
            }
            ';' | '\n' if !in_single && !in_double => {
                cut_command(line, i, i + c.len_utf8(), &mut commands, &mut start);
            }
            // `&&` ends the command as well; the `&` of `2>&1` does not.
            '&' if !in_single && !in_double && prev != '>' && prev != '<' => {
                let width = if chars.peek().is_some_and(|(_, n)| *n == '&') {
                    chars.next();
                    2
                } else {
                    1
                };
                cut_command(line, i, i + width, &mut commands, &mut start);
            }
            '|' if !in_single && !in_double => {
                let width = if chars.peek().is_some_and(|(_, n)| *n == '|' || *n == '&') {
                    chars.next();
                    2
                } else {
                    1
                };
                cut_command(line, i, i + width, &mut commands, &mut start);
            }
            _ => {}
        }
        prev = c;
    }
    commands.push(&line[start..]);
    commands.extend(nested);
    commands
}

fn cut_command<'a>(
    line: &'a str,
    from: usize,
    to: usize,
    commands: &mut Vec<&'a str>,
    start: &mut usize,
) {
    commands.push(&line[*start..from]);
    *start = to;
}

/// Index just past the next character `ends_with`, or `end` if there is none.
fn skip_past(
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    end: usize,
    ends_with: impl Fn(char) -> bool,
) -> usize {
    while let Some((i, c)) = chars.next() {
        if c == '\\' {
            chars.next();
            continue;
        }
        if ends_with(c) {
            return i + c.len_utf8();
        }
    }
    end
}

/// Index just past the `)` closing a `$(…)` whose `(` is already consumed.
fn skip_parens(chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>, end: usize) -> usize {
    let mut depth = 1usize;
    let mut in_single = false;
    let mut in_double = false;
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' if !in_single => {
                chars.next();
            }
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' if !in_single => depth += 1,
            ')' if !in_single => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
    }
    end
}

/// Adjacent `message` `send` words in text that is not a plain argv — a
/// redirection, an unbalanced quote. Whitespace and shell separators (`;`
/// `|` `&`) split it.
///
/// A quoted run that holds a separator is text — `echo "message send" > log`
/// is not a send. Quotes around one word (`neomind "message" send`) are the
/// word. `raw` ignores quoting altogether, for a line nested too deep to
/// judge and for a program that runs whatever its argument says.
fn message_send_words_in(text: &str, raw: bool) -> bool {
    // (lowercased word, is quoted text)
    let mut words: Vec<(String, bool)> = Vec::new();
    let mut word = String::new();
    let mut quoted_text = false;
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if !in_single && !raw => {
                if let Some(next) = chars.next() {
                    word.push(next);
                }
            }
            '\'' if !raw && !in_double => in_single = !in_single,
            '"' if !raw && !in_single => in_double = !in_double,
            c if (raw || !in_single && !in_double) && is_word_break(c) => {
                flush_word(&mut words, &mut word, &mut quoted_text);
            }
            c => {
                if !raw && (in_single || in_double) && is_word_break(c) {
                    quoted_text = true;
                }
                word.push(c);
            }
        }
    }
    flush_word(&mut words, &mut word, &mut quoted_text);
    words.windows(2).any(|pair| {
        pair[0].0 == "message" && pair[1].0 == "send" && (raw || !pair[0].1 && !pair[1].1)
    })
}

fn is_word_break(c: char) -> bool {
    c.is_whitespace() || matches!(c, ';' | '|' | '&')
}

fn flush_word(words: &mut Vec<(String, bool)>, word: &mut String, quoted_text: &mut bool) {
    if !word.is_empty() {
        // In raw mode the quotes themselves are part of the word.
        let word = std::mem::take(word);
        words.push((
            word.trim_matches(['"', '\'']).to_ascii_lowercase(),
            std::mem::take(quoted_text),
        ));
    }
}

/// After tool_loop rewrite, return the shell command string if this call would
/// deliver a platform message (`message send`). Shared by direct `shell` and
/// rewritten CLI-domain (`message`, …) entry points.
pub(crate) fn pending_message_send_command(tool_name: &str, args: &Value) -> Option<String> {
    if tool_name != "shell" {
        return None;
    }
    let command = args.get("command").and_then(|v| v.as_str())?;
    if is_message_send_command(command) {
        Some(command.to_string())
    } else {
        None
    }
}

/// Latest non-empty assistant prose for this event, with the tool-call
/// payload removed (see [`assistant_prose`]).
/// Deliberately ignores pending send title/body and older turns.
pub(crate) fn judgment_context_from_messages(messages: &[Message]) -> String {
    messages
        .iter()
        .rev()
        .filter(|m| m.role == MessageRole::Assistant)
        .filter_map(|m| {
            let text = m.content.as_text();
            let prose = assistant_prose(&text);
            (!prose.trim().is_empty()).then(|| prose.into_owned())
        })
        .next()
        .unwrap_or_default()
}

/// Assistant text with the tool-call payload removed, so the verdict is read
/// from the model's own words and never from the serialized call.
///
/// Every backend appends the native tool-call array to the response text
/// (ollama.rs / openai runtime.rs / llamacpp.rs "backward compat", no
/// separator), and text-protocol models inline `<tool_calls>` / Hermes /
/// qwen-coder syntax. Either lands after the verdict line, so the raw last
/// line was `[{"id":…}]` and the gate denied every send — including a
/// verdict that followed the protocol exactly.
fn assistant_prose(text: &str) -> std::borrow::Cow<'_, str> {
    if let Some(prose) = strip_trailing_native_tool_calls(text) {
        return std::borrow::Cow::Borrowed(prose);
    }
    match crate::agent::tool_parser::parse_tool_calls(text) {
        Ok((rest, calls)) if !calls.is_empty() => std::borrow::Cow::Owned(rest),
        _ => std::borrow::Cow::Borrowed(text),
    }
}

/// Remove the serialized native tool-call array a backend appends to the
/// response text. Its start is the last `[{"` from which a tool-call array
/// parses through to the very end — not simply the last `[`, which may
/// belong to a nested list inside the call arguments. Up to 256 `[` from the
/// end are tried: an alert body with a bracket of its own must not hide the
/// verdict and cost a justified alert.
fn strip_trailing_native_tool_calls(text: &str) -> Option<&str> {
    text.rmatch_indices('[')
        .take(256)
        .filter_map(|(pos, _)| text[pos..].strip_prefix("[{\"").map(|_| pos))
        .find_map(|pos| {
            let tail = &text[pos..];
            let calls = serde_json::from_str::<Vec<Value>>(tail).ok()?;
            let is_tool_calls = !calls.is_empty()
                && calls.iter().all(|call| {
                    call.is_object()
                        && (call.get("name").is_some() || call.get("function").is_some())
                });
            is_tool_calls.then(|| &text[..pos])
        })
}

/// Verdict for the gate: parse a finished judgment label from the latest
/// independent assistant message only.
pub(crate) fn judgment_verdict_from_messages(messages: &[Message]) -> JudgmentVerdict {
    classify_judgment(&judgment_context_from_messages(messages))
}

/// Classify text from the **last non-empty line** via exact `Verdict:` protocol.
///
/// Mid-message examples (including backtick-wrapped prompt samples) and
/// explanatory mentions never authorize.
pub(crate) fn classify_judgment(text: &str) -> JudgmentVerdict {
    let Some(last) = text.lines().rev().find(|l| !l.trim().is_empty()) else {
        return JudgmentVerdict::Unknown;
    };
    parse_verdict_line(last).unwrap_or(JudgmentVerdict::Unknown)
}

/// Exact last-line protocol. No backtick stripping; no bare labels.
fn parse_verdict_line(line: &str) -> Option<JudgmentVerdict> {
    let mut t = line.trim();
    if t.is_empty() {
        return None;
    }
    // Trailing sentence punctuation only — do not strip backticks/bold
    // (quoted examples must not authorize).
    t = t.trim_end_matches(['.', '。', '!', '！']);
    let lower = t.to_ascii_lowercase();
    let rest = lower.strip_prefix("verdict:")?;
    match rest.trim() {
        "criteria met" => Some(JudgmentVerdict::CriteriaMet),
        "criteria not met" => Some(JudgmentVerdict::CriteriaNotMet),
        "insufficient evidence" => Some(JudgmentVerdict::InsufficientEvidence),
        _ => None,
    }
}

/// Only an explicit finished "criteria met" judgment may deliver a notification.
pub(crate) fn judgment_allows_send(verdict: JudgmentVerdict) -> bool {
    matches!(verdict, JudgmentVerdict::CriteriaMet)
}

/// Leading marker of every judgment-gate deny reason. The tool loop keys on
/// it to tell a gate block from a safety-policy block.
pub(crate) const JUDGMENT_GATE_REASON_PREFIX: &str = "Judgment notify gate:";

/// Human-readable block reason returned to the model (and tests).
pub(crate) fn gate_block_reason(verdict: JudgmentVerdict) -> String {
    match verdict {
        JudgmentVerdict::CriteriaNotMet => format!(
            "{JUDGMENT_GATE_REASON_PREFIX} alert criteria not met — notification blocked \
             (silence is the correct outcome)."
        ),
        JudgmentVerdict::InsufficientEvidence => format!(
            "{JUDGMENT_GATE_REASON_PREFIX} insufficient evidence to decide — notification \
             blocked."
        ),
        JudgmentVerdict::Unknown => format!(
            "{JUDGMENT_GATE_REASON_PREFIX} no valid judgment yet (need criteria met / not \
             met / insufficient evidence) — notification blocked."
        ),
        JudgmentVerdict::CriteriaMet => {
            // Caller should not block on CriteriaMet; keep a defensive string.
            format!("{JUDGMENT_GATE_REASON_PREFIX} criteria met")
        }
    }
}

/// Extra vision-aware context for the Judgment send gate (same-batch +
/// ViaVisionTool blind-send races beyond prior-round failure).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct JudgmentVisionGate<'a> {
    /// Other tool names in the same execute batch (post-rewrite).
    pub same_batch_names: &'a [&'a str],
    /// True when this run requires the vision tool (images + ViaVisionTool).
    pub vision_required: bool,
}

/// Shared tool-loop gate for every Judgment delivery call that reaches shell
/// (direct or rewritten). Returns a deny reason when send must be blocked.
///
/// Closes three Partial races:
/// 1. prior rounds already vision-all-failed
/// 2. same-batch vision + message send (gate runs before the batch executes)
/// 3. ViaVisionTool blind send (images present, no successful vision yet)
pub(crate) fn judgment_deny_for_tool_call(
    agent: &AiAgent,
    tool_name: &str,
    args: &Value,
    messages: &[Message],
    prior_tool_results: &[crate::toolkit::ToolResult],
    vision_gate: JudgmentVisionGate<'_>,
) -> Option<String> {
    if !agent_uses_judgment_notify(agent) {
        return None;
    }
    let _command = pending_message_send_command(tool_name, args)?;
    if vision_tools_already_failed(prior_tool_results) {
        return Some(format!(
            "{JUDGMENT_GATE_REASON_PREFIX} vision tools already failed this run — \
             notification blocked (undetermined / Partial)."
        ));
    }
    if vision_gate
        .same_batch_names
        .iter()
        .any(|n| super::vision_path::is_vision_tool_name(n))
    {
        return Some(format!(
            "{JUDGMENT_GATE_REASON_PREFIX} vision tool is still running in this \
             batch — notification blocked until vision completes."
        ));
    }
    if vision_gate.vision_required && !vision_tools_any_success(prior_tool_results) {
        return Some(format!(
            "{JUDGMENT_GATE_REASON_PREFIX} vision not yet confirmed this run — \
             notification blocked (need a successful vision call first)."
        ));
    }
    let verdict = judgment_verdict_from_messages(messages);
    if judgment_allows_send(verdict) {
        None
    } else {
        Some(gate_block_reason(verdict))
    }
}

/// True when at least one vision tool already ran this loop and every vision
/// call failed — same semantics as [`super::vision_path::vision_tools_all_failed`].
fn vision_tools_already_failed(results: &[crate::toolkit::ToolResult]) -> bool {
    let mut saw_vision = false;
    for r in results {
        if !super::vision_path::is_vision_tool_name(&r.name) {
            continue;
        }
        saw_vision = true;
        let success = matches!(
            &r.result,
            Ok(o) if crate::toolkit::tool::effective_success(&r.name, o)
        );
        if success {
            return false;
        }
    }
    saw_vision
}

fn vision_tools_any_success(results: &[crate::toolkit::ToolResult]) -> bool {
    results.iter().any(|r| {
        super::vision_path::is_vision_tool_name(&r.name)
            && matches!(
                &r.result,
                Ok(o) if crate::toolkit::tool::effective_success(&r.name, o)
            )
    })
}

/// Convenience for tests that already hold a shell command string.
#[cfg(test)]
pub(crate) fn judgment_send_deny_reason(
    agent: &AiAgent,
    command: &str,
    messages: &[Message],
) -> Option<String> {
    let args = serde_json::json!({ "command": command });
    judgment_deny_for_tool_call(
        agent,
        "shell",
        &args,
        messages,
        &[],
        JudgmentVisionGate::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use neomind_storage::{
        AgentMemory, AgentNotify, AgentSchedule, AgentStats, AgentStatus, ExecutionJournal,
        ExecutionMode, ScheduleType,
    };

    fn agent_with_notify(on: NotifyOn) -> AiAgent {
        let now = 0_i64;
        AiAgent {
            id: "j-test".into(),
            name: "J".into(),
            description: None,
            user_prompt: "test".into(),
            llm_backend_id: None,
            parsed_intent: None,
            resources: vec![],
            schedule: AgentSchedule {
                schedule_type: ScheduleType::Manual,
                interval_seconds: None,
                cron_expression: None,
                timezone: None,
                event_filter: None,
            },
            status: AgentStatus::Active,
            priority: 128,
            created_at: now,
            updated_at: now,
            last_execution_at: None,
            stats: AgentStats {
                total_executions: 0,
                successful_executions: 0,
                failed_executions: 0,
                avg_duration_ms: 0,
                last_duration_ms: None,
            },
            memory: AgentMemory {
                journal: ExecutionJournal::default(),
                knowledge_files: vec![],
                updated_at: now,
            },
            conversation_history: vec![],
            user_messages: vec![],
            conversation_summary: None,
            context_window_size: 5,
            tool_config: None,
            execution_mode: ExecutionMode::Free,
            error_message: None,
            system_prompt: None,
            max_retries: 0,
            consecutive_failures: 0,
            output_schema: None,
            operator_config: None,
            memory_mode: None,
            notify: Some(AgentNotify {
                channels: vec!["webhook:ops".into()],
                on,
            }),
            enable_tool_chaining: false,
            max_chain_depth: 3,
            max_tokens_per_run: None,
        }
    }

    #[test]
    fn classifies_last_line_verdict_protocol_only() {
        assert_eq!(
            classify_judgment("corridor empty.\nVerdict: criteria not met"),
            JudgmentVerdict::CriteriaNotMet
        );
        assert_eq!(
            classify_judgment("有人且疑似赌博。\nVerdict: criteria met"),
            JudgmentVerdict::CriteriaMet
        );
        assert_eq!(
            classify_judgment("画面过暗。\nVerdict: insufficient evidence"),
            JudgmentVerdict::InsufficientEvidence
        );
        assert_eq!(
            classify_judgment("looking around"),
            JudgmentVerdict::Unknown
        );
        // Bare label / bold / backticks never authorize.
        assert_eq!(classify_judgment("criteria met"), JudgmentVerdict::Unknown);
        assert_eq!(
            classify_judgment("**criteria met**"),
            JudgmentVerdict::Unknown
        );
        assert_eq!(
            classify_judgment("`Verdict: criteria met`"),
            JudgmentVerdict::Unknown
        );
        // Lexicon / prose alone never authorizes.
        assert_eq!(
            classify_judgment("有人且疑似赌博，应告警。"),
            JudgmentVerdict::Unknown
        );
        assert_eq!(classify_judgment("需要告警"), JudgmentVerdict::Unknown);
        assert_eq!(
            classify_judgment("I have not yet determined whether the criteria met label applies."),
            JudgmentVerdict::Unknown
        );
        // Mid-message quoted example + later prose → last line is not protocol.
        assert_eq!(
            classify_judgment(
                "阳性判定的格式示例如下：\n`Verdict: criteria met`\n本帧还在分析中。"
            ),
            JudgmentVerdict::Unknown
        );
    }

    #[test]
    fn question_containing_need_alert_is_not_finished_judgment() {
        assert_eq!(classify_judgment("需要告警吗？"), JudgmentVerdict::Unknown);
        assert_eq!(classify_judgment("criteria met?"), JudgmentVerdict::Unknown);
    }

    #[test]
    fn last_non_empty_line_is_the_only_protocol_position() {
        // Earlier Verdict lines do not authorize when the last line is prose.
        assert_eq!(
            classify_judgment("Verdict: criteria met\n本帧还在分析中。"),
            JudgmentVerdict::Unknown
        );
        // Last protocol line wins.
        assert_eq!(
            classify_judgment("Verdict: criteria met\nVerdict: insufficient evidence"),
            JudgmentVerdict::InsufficientEvidence
        );
        assert_eq!(
            classify_judgment("Verdict: criteria met\nVerdict: criteria not met"),
            JudgmentVerdict::CriteriaNotMet
        );
        assert_eq!(
            classify_judgment("criteria met but insufficient evidence overall"),
            JudgmentVerdict::Unknown
        );
    }

    #[test]
    fn detects_message_send_variants() {
        for cmd in [
            r#"neomind message send --title "t" --body "b""#,
            r#"message send --title "t" --body "b""#,
            // Same argv once the shell tokenizer runs — must not read as a
            // different command to the gate.
            r#"neomind  message   send --title "t" --body "b""#,
            "neomind\tmessage\tsend --title t --body b",
            r#"neomind "message" send --title t --body b"#,
            r#"neomind mes\sage send --title t"#,
            // A send wrapped in a real shell line.
            r#"neomind device get cam-1; neomind message send --title t --body b"#,
            r#"neomind device get cam-1 && neomind  message   send --title t"#,
            r#"neomind message send --title t --body b 2>&1 | head -5"#,
            r#"NEOMIND_JSON=1 neomind message send --title t --body b"#,
            r#"neomind "message" send --title t; neomind device list"#,
            // A command line quoted inside another one: the shell tool hands
            // these to /bin/sh, which runs the send.
            r#"sh -c "neomind message send --title t --body b""#,
            "bash -c 'neomind message send --title t --body b'",
            "sh -lc 'neomind message send --title t'",
            r#"eval "neomind message send --title t""#,
            r#"sudo neomind message send --title t"#,
            r#"sudo -u neo neomind message send --title t"#,
            r#"NEOMIND_JSON=1 timeout 30 neomind message send --title t"#,
            r#"/usr/local/bin/neomind message send --title t"#,
            // Shell syntax around the command.
            r#"(neomind message send --title t)"#,
            "{ neomind message send --title t; }",
            "if true; then neomind message send --title t; fi",
            "neomind message send --title t &",
            // Substitutions run as commands of their own, quotes or not.
            r#"echo "$(neomind message send --title t)""#,
            "echo `neomind message send --title t`",
            r#""$(which neomind)" message send --title t"#,
            // A real send with shell characters of its own inside quotes.
            r#"neomind message send --title "a | b; c" --body "x""#,
            r#"neomind message send --title t --body "message send failed""#,
            r#"neomind message send --title t > out.txt 2>&1"#,
            // Redirection keeps the words quoted one by one.
            r#"neomind "message" send --title t 2>&1 | head -5"#,
            // A program that runs whatever its argument says.
            r#"python3 -c "import os; os.system('neomind message send --title t')""#,
        ] {
            assert!(is_message_send_command(cmd), "must gate: {cmd:?}");
        }
    }

    /// Parameter text that quotes the command's words is text. Judged by
    /// argv, a quoted argument is one token — it used to be split on
    /// whitespace, so an ordinary command whose text mentioned "message
    /// send" was stopped until the model produced a verdict.
    #[test]
    fn quoted_parameter_text_is_not_a_send() {
        for cmd in [
            r#"neomind device control 20545d67 display --params '{"text":"message send failed"}'"#,
            r#"neomind device control 20545d67 display --params {"text":"message send failed"}"#,
            r#"neomind device get 200366 --output "x | y; z""#,
            r#"neomind agent update a1 --user-prompt "when in doubt, message send nothing""#,
            r#"neomind message list --query "message send""#,
            r#"neomind device get cam-1 && neomind device get "message send""#,
            r#"neomind device get cam-1; echo "message send""#,
            r#"echo "message send""#,
            r#"echo "neomind message send""#,
            r#"grep -r "message send" /var/log/neomind"#,
            r#"echo "message send" > notes.txt"#,
            r#"echo "please message the send team""#,
            "neomind message list",
            "neomind device get cam-1",
            r#"neomind device get cam-1; neomind message list"#,
            r#"sh -c "echo message send""#,
            r#"sh deploy.sh"#,
            r#"echo "$(cat notes.txt) message send""#,
        ] {
            assert!(!is_message_send_command(cmd), "must not gate: {cmd:?}");
        }
    }

    /// The pair at the gate itself, with no verdict anywhere in the
    /// conversation: an ordinary command runs, a real send is stopped — and
    /// goes through once the verdict is there.
    #[test]
    fn ordinary_text_passes_the_gate_and_a_real_send_needs_a_verdict() {
        let agent = agent_with_notify(NotifyOn::Judgment);
        let ordinary =
            r#"neomind device control 20545d67 display --params '{"text":"message send failed"}'"#;
        let send = r#"neomind message send --title "有人" --body "两人打麻将""#;

        assert!(
            judgment_send_deny_reason(&agent, ordinary, &[]).is_none(),
            "parameter text is not a send"
        );
        assert_eq!(
            judgment_send_deny_reason(&agent, send, &[]),
            Some(gate_block_reason(JudgmentVerdict::Unknown)),
            "a send without a verdict is stopped"
        );

        let verdict = [Message::new(
            MessageRole::Assistant,
            "画面里两人打麻将。\nVerdict: criteria met",
        )];
        assert!(
            judgment_send_deny_reason(&agent, send, &verdict).is_none(),
            "with the verdict the send goes through"
        );
    }

    /// Mirrors the real backends (ollama.rs / openai runtime.rs /
    /// llamacpp.rs): the native tool-call array is appended to the response
    /// text with no separator.
    fn backend_appended(prose: &str, call: serde_json::Value) -> String {
        format!(
            "{prose}{}",
            serde_json::to_string(&vec![call]).unwrap_or_default()
        )
    }

    fn shell_call(arguments: serde_json::Value) -> serde_json::Value {
        serde_json::json!({ "id": "call_1", "name": "shell", "arguments": arguments })
    }

    fn verdict_of(text: &str) -> JudgmentVerdict {
        judgment_verdict_from_messages(&[Message::new(MessageRole::Assistant, text)])
    }

    #[test]
    fn verdict_is_read_before_the_appended_tool_call_array() {
        let send = || {
            shell_call(serde_json::json!({
                "command": "neomind message send --title \"Alert\" --body \"details\""
            }))
        };
        let met = [
            backend_appended("有人且疑似赌博。\nVerdict: criteria met", send()),
            backend_appended("有人且疑似赌博。\nVerdict: criteria met\n", send()),
            // Prose that itself contains `[`.
            backend_appended("设备 [200366] 有人。\nVerdict: criteria met", send()),
            // A nested list inside the call arguments: its `[` is the LAST one.
            backend_appended(
                "有人。\nVerdict: criteria met",
                shell_call(serde_json::json!({
                    "command": "neomind message send --title t",
                    "tags": ["a", "b"],
                })),
            ),
            // An alert body with far more than 64 brackets of its own.
            backend_appended(
                "有人。\nVerdict: criteria met",
                shell_call(serde_json::json!({
                    "command": format!(
                        "neomind message send --title t --body \"{}\"",
                        "[位]".repeat(100)
                    ),
                })),
            ),
        ];
        for text in met {
            assert_eq!(
                verdict_of(&text),
                JudgmentVerdict::CriteriaMet,
                "appended tool-call array must not hide the verdict: {text}"
            );
        }
        // Deny verdicts stay deny; a call with no model text is still no verdict.
        assert_eq!(
            verdict_of(&backend_appended(
                "画面为空旷走廊。\nVerdict: criteria not met",
                send()
            )),
            JudgmentVerdict::CriteriaNotMet
        );
        assert_eq!(
            verdict_of(&backend_appended("", send())),
            JudgmentVerdict::Unknown
        );
    }

    #[test]
    fn verdict_is_read_around_inline_text_protocol_tool_calls() {
        // Text-protocol models inline the call; either order must resolve.
        for text in [
            concat!(
                "有人且疑似赌博。\nVerdict: criteria met\n",
                r#"<tool_calls><invoke name="shell"><parameter name="command">"#,
                r#"neomind message send --title t</parameter></invoke></tool_calls>"#,
            ),
            concat!(
                r#"<tool_calls><invoke name="shell"><parameter name="command">"#,
                r#"neomind message send --title t</parameter></invoke></tool_calls>"#,
                "\n有人且疑似赌博。\nVerdict: criteria met",
            ),
            concat!(
                "有人且疑似赌博。\nVerdict: criteria met\n",
                r#"<function name="shell">"#,
                r#"<parameter name="command">neomind message send --title t</parameter></function>"#,
            ),
        ] {
            assert_eq!(
                verdict_of(text),
                JudgmentVerdict::CriteriaMet,
                "inline tool-call syntax must not hide the verdict: {text}"
            );
        }
        // Prose that merely ends in a bracket list is left alone.
        assert_eq!(
            verdict_of("本帧还在分析中。\n[{\"note\":\"not a call\"}]"),
            JudgmentVerdict::Unknown
        );
    }

    #[test]
    fn title_body_are_not_merged_into_judgment_context() {
        let msgs = vec![Message::new(MessageRole::Assistant, "巡检中。")];
        let ctx = judgment_context_from_messages(&msgs);
        assert_eq!(ctx, "巡检中。");
        assert!(
            !ctx.contains("疑似赌博"),
            "context must not invent title text"
        );
    }

    #[test]
    fn pending_title_alone_cannot_self_approve() {
        let agent = agent_with_notify(NotifyOn::Judgment);
        let cmd = r#"neomind message send --title "疑似赌博" --body "告警""#;
        let msgs: Vec<Message> = vec![];
        let blocked = judgment_send_deny_reason(&agent, cmd, &msgs);
        assert!(
            blocked
                .as_ref()
                .is_some_and(|r| r.contains("no valid judgment")),
            "title must not self-approve; got {blocked:?}"
        );
    }

    #[test]
    fn policy_conditional_is_not_a_this_event_judgment() {
        assert_eq!(
            classify_judgment("如果发现赌博，就需要告警"),
            JudgmentVerdict::Unknown
        );
        assert_eq!(
            classify_judgment("规则：如果发现赌博，就需要告警。"),
            JudgmentVerdict::Unknown
        );
        // Free-text finding without finished label is still Unknown.
        assert_eq!(
            classify_judgment("有人且疑似赌博，应告警。"),
            JudgmentVerdict::Unknown
        );
        // Policy noise + protocol verdict line → met.
        assert_eq!(
            classify_judgment("规则：如果发现赌博，就需要告警。本帧有人。\nVerdict: criteria met"),
            JudgmentVerdict::CriteriaMet
        );
    }

    #[test]
    fn progress_intent_is_not_a_finished_judgment() {
        assert_eq!(
            classify_judgment("我先核对画面是否需要告警"),
            JudgmentVerdict::Unknown
        );
        assert_eq!(
            classify_judgment("让我检查一下是否需要告警。"),
            JudgmentVerdict::Unknown
        );
        assert_eq!(
            classify_judgment("我先核对画面是否需要告警。有人且疑似赌博。\nVerdict: criteria met"),
            JudgmentVerdict::CriteriaMet
        );
        let agent = agent_with_notify(NotifyOn::Judgment);
        let cmd = r#"neomind message send --title "x" --body "y""#;
        let msgs = vec![Message::new(
            MessageRole::Assistant,
            "我先核对画面是否需要告警",
        )];
        assert!(
            judgment_send_deny_reason(&agent, cmd, &msgs)
                .as_ref()
                .is_some_and(|r| r.contains("no valid judgment")),
            "progress text must not authorize send"
        );
    }

    #[test]
    fn prior_inconclusive_must_not_override_this_event_positive() {
        let msgs = vec![
            Message::new(
                MessageRole::Assistant,
                "画面过暗。\nVerdict: insufficient evidence",
            ),
            Message::new(
                MessageRole::Assistant,
                "有人且疑似赌博。\nVerdict: criteria met",
            ),
        ];
        assert_eq!(
            judgment_verdict_from_messages(&msgs),
            JudgmentVerdict::CriteriaMet
        );
        let agent = agent_with_notify(NotifyOn::Judgment);
        let cmd = r#"neomind message send --title "Alert" --body "details""#;
        assert!(
            judgment_send_deny_reason(&agent, cmd, &msgs).is_none(),
            "prior dark conclusion must not block this event's positive judgment"
        );
    }

    #[test]
    fn prior_positive_must_not_authorize_policy_only_send() {
        let msgs = vec![
            Message::new(
                MessageRole::Assistant,
                "有人且疑似赌博。\nVerdict: criteria met",
            ),
            Message::new(MessageRole::Assistant, "如果发现赌博，就需要告警"),
        ];
        assert_eq!(
            judgment_verdict_from_messages(&msgs),
            JudgmentVerdict::Unknown
        );
        let agent = agent_with_notify(NotifyOn::Judgment);
        let cmd = r#"neomind message send --title "疑似赌博" --body "x""#;
        assert!(
            judgment_send_deny_reason(&agent, cmd, &msgs)
                .as_ref()
                .is_some_and(|r| r.contains("no valid judgment")),
            "prior met + policy-only latest must not authorize send"
        );
    }

    #[test]
    fn gate_blocks_not_met_for_judgment_agents_only() {
        let judgment = agent_with_notify(NotifyOn::Judgment);
        let always = agent_with_notify(NotifyOn::Always);
        let cmd = r#"neomind message send --title "x" --body "y""#;
        let msgs = vec![Message::new(
            MessageRole::Assistant,
            "巡检完成：无赌博。\nVerdict: criteria not met",
        )];

        let blocked = judgment_send_deny_reason(&judgment, cmd, &msgs);
        assert!(
            blocked
                .as_ref()
                .is_some_and(|r| r.contains("criteria not met")),
            "got: {blocked:?}"
        );

        assert!(
            judgment_send_deny_reason(&always, cmd, &msgs).is_none(),
            "Always notify must keep the old contract"
        );
    }

    #[test]
    fn gate_allows_criteria_met_from_assistant_only() {
        let agent = agent_with_notify(NotifyOn::Judgment);
        let cmd = r#"neomind message send --title "Alert" --body "details""#;
        let msgs = vec![Message::new(
            MessageRole::Assistant,
            "有人且疑似赌博。\nVerdict: criteria met",
        )];
        assert!(judgment_send_deny_reason(&agent, cmd, &msgs).is_none());
    }

    #[test]
    fn question_need_alert_cannot_authorize_send() {
        let agent = agent_with_notify(NotifyOn::Judgment);
        let cmd = r#"neomind message send --title "x" --body "y""#;
        let msgs = vec![Message::new(MessageRole::Assistant, "需要告警吗？")];
        assert!(
            judgment_send_deny_reason(&agent, cmd, &msgs)
                .as_ref()
                .is_some_and(|r| r.contains("no valid judgment")),
            "open question must not authorize"
        );
    }

    #[test]
    fn explanatory_mention_of_criteria_met_cannot_authorize_send() {
        let agent = agent_with_notify(NotifyOn::Judgment);
        let cmd = r#"neomind message send --title "x" --body "y""#;
        let probe = "I have not yet determined whether the criteria met label applies.";
        assert_eq!(classify_judgment(probe), JudgmentVerdict::Unknown);
        let msgs = vec![Message::new(MessageRole::Assistant, probe)];
        assert!(
            judgment_send_deny_reason(&agent, cmd, &msgs)
                .as_ref()
                .is_some_and(|r| r.contains("no valid judgment")),
            "explanatory contains() must not authorize"
        );
        assert!(!judgment_allows_send(classify_judgment(probe)));
    }

    #[test]
    fn direct_shell_and_rewritten_cli_share_same_gate() {
        let agent = agent_with_notify(NotifyOn::Judgment);
        let msgs: Vec<Message> = vec![];
        let direct = serde_json::json!({
            "command": "neomind message send --title \"疑似赌博\" --body \"x\""
        });
        let rewritten = crate::tools::mapper::build_cli_command(
            "message",
            &serde_json::json!({ "title": "疑似赌博", "content": "x" }),
        )
        .expect("message domain rewrites to shell");
        assert_eq!(
            rewritten
                .get("command")
                .and_then(|v| v.as_str())
                .map(is_message_send_command),
            Some(true),
            "rewritten CLI must be a message send: {rewritten}"
        );

        let gate = JudgmentVisionGate::default();
        let d = judgment_deny_for_tool_call(&agent, "shell", &direct, &msgs, &[], gate);
        let r = judgment_deny_for_tool_call(&agent, "shell", &rewritten, &msgs, &[], gate);
        assert!(d.is_some() && r.is_some(), "both entry points must deny");
        assert_eq!(d, r, "direct shell and rewritten CLI must share one gate");

        assert!(JUDGMENT_NOTIFY_ENTRY_POINTS
            .iter()
            .any(|e| e.contains("shell.message_send")));
        assert!(JUDGMENT_NOTIFY_ENTRY_POINTS
            .iter()
            .any(|e| e.contains("rewritten_cli_domain")));
    }

    #[test]
    fn non_send_shell_is_not_gated() {
        let agent = agent_with_notify(NotifyOn::Judgment);
        let args = serde_json::json!({ "command": "neomind device get cam-1" });
        assert!(judgment_deny_for_tool_call(
            &agent,
            "shell",
            &args,
            &[],
            &[],
            JudgmentVisionGate::default()
        )
        .is_none());
    }

    /// Mid-loop Partial race: vision tools already all failed → deny send even
    /// with a finished `Verdict: criteria met` (finish path has not marked
    /// Partial yet).
    #[test]
    fn vision_already_failed_blocks_criteria_met_send() {
        let agent = agent_with_notify(NotifyOn::Judgment);
        let msgs = vec![Message::new(
            MessageRole::Assistant,
            "有人且疑似赌博。\nVerdict: criteria met",
        )];
        let args = serde_json::json!({
            "command": "neomind message send --title \"告警\" --body \"疑似赌博\""
        });
        let gate = JudgmentVisionGate::default();
        assert!(
            judgment_deny_for_tool_call(&agent, "shell", &args, &msgs, &[], gate).is_none(),
            "without prior vision failure, criteria met must allow"
        );

        let prior = vec![crate::toolkit::ToolResult {
            name: "vision".into(),
            result: Ok(crate::toolkit::ToolOutput {
                success: false,
                data: serde_json::json!({"error": "timeout"}),
                error: Some("timeout".into()),
                metadata: None,
            }),
        }];
        let denied = judgment_deny_for_tool_call(&agent, "shell", &args, &msgs, &prior, gate);
        assert!(
            denied
                .as_ref()
                .is_some_and(|r| r.contains("vision tools already failed")),
            "vision-all-failed must block mid-loop send: {denied:?}"
        );
    }

    #[test]
    fn same_batch_vision_blocks_criteria_met_send() {
        let agent = agent_with_notify(NotifyOn::Judgment);
        let msgs = vec![Message::new(
            MessageRole::Assistant,
            "有人。\nVerdict: criteria met",
        )];
        let args = serde_json::json!({
            "command": "neomind message send --title \"告警\" --body \"疑似\""
        });
        let peers = ["vision", "shell"];
        let denied = judgment_deny_for_tool_call(
            &agent,
            "shell",
            &args,
            &msgs,
            &[],
            JudgmentVisionGate {
                same_batch_names: &peers,
                vision_required: true,
            },
        );
        assert!(
            denied
                .as_ref()
                .is_some_and(|r| r.contains("still running in this batch")),
            "same-batch vision+send must block: {denied:?}"
        );
    }

    #[test]
    fn vision_required_without_success_blocks_blind_send() {
        let agent = agent_with_notify(NotifyOn::Judgment);
        let msgs = vec![Message::new(
            MessageRole::Assistant,
            "现场正常。\nVerdict: criteria met",
        )];
        let args = serde_json::json!({
            "command": "neomind message send --title \"正常\" --body \"无异常\""
        });
        let denied = judgment_deny_for_tool_call(
            &agent,
            "shell",
            &args,
            &msgs,
            &[],
            JudgmentVisionGate {
                same_batch_names: &[],
                vision_required: true,
            },
        );
        assert!(
            denied
                .as_ref()
                .is_some_and(|r| r.contains("vision not yet confirmed")),
            "blind ViaVisionTool send must block: {denied:?}"
        );
    }
}
