// ---------------------------------------------------------------------------
// "List-only dead end" detection helpers
// ---------------------------------------------------------------------------

/// Action verbs (in Chinese and English) that indicate the user wants a mutation,
/// not just a query. When the original user message contains these but all executed
/// tool calls were read-only (list/get/latest/history), we detect a "list-only dead end"
/// and inject a forced continuation prompt.
const ACTION_VERBS: &[&str] = &[
    // Chinese
    "创建",
    "新建",
    "删除",
    "控制",
    "启用",
    "禁用",
    "启动",
    "停止",
    "更新",
    "修改",
    "开启",
    "关闭",
    "打开",
    "发送",
    "写入",
    "分享",
    "安装",
    "卸载",
    "移除",
    "批量启用",
    "批量删除",
    "批量创建",
    "全部启动",
    "添加",
    "替换",
    "绑定",
    "巡检",
    // English
    "create",
    "delete",
    "control",
    "enable",
    "disable",
    "start",
    "stop",
    "update",
    "turn on",
    "turn off",
    "switch",
    "send",
    "write",
    "share",
    "install",
    "uninstall",
    "remove",
    "add",
    "replace",
    "bind",
];

/// Check if the user message requests a mutation/action (not just a query).
pub(crate) fn user_message_requires_action(msg: &str) -> bool {
    let msg_lower = msg.to_lowercase();
    ACTION_VERBS.iter().any(|verb| msg_lower.contains(verb))
}

/// Check if ALL executed tool calls so far were read-only (list/get/query).
/// Takes the actual shell command strings (not tool names) for accurate detection.
/// Returns true if no mutation command was found in any tool call.
pub(crate) fn all_tools_were_read_only(
    executed_commands: &[&str],
    _all_results: &[(String, String)],
) -> bool {
    // Single source: the shared MUTATION_COMMAND_PATTERNS. This function
    // used to carry its OWN near-copy of the list, and the two drifted —
    // the read-only detector kept the buggy bare " push" (every `neomind
    // push list` read as a mutation, dead-ending dead-end detection for
    // the push domain) and never learned market-install/start/stop/…,
    // so list-only continuations fired after real mutations ran.
    //
    // If no commands were executed, we can't determine — assume not read-only
    if executed_commands.is_empty() {
        return false;
    }

    for cmd in executed_commands {
        if is_mutation_command(&cmd.to_lowercase()) {
            return false;
        }
    }

    // All commands were read-only (list/get/latest/history/etc.)
    true
}

/// Build the forced-continuation prompt for the "list-only dead end" case.
///
/// This is shared between the text-only path (`stream_core.rs`) and the
/// multimodal path (`stream_multimodal.rs`) so the prompt stays consistent.
///
/// Returns `Some(prompt)` if the user asked for an action but only read-only
/// tools were executed, otherwise `None` (caller proceeds with the normal
/// continuation prompt).
///
/// Arguments:
/// - `user_message`: original user message text
/// - `executed_commands`: actual shell command strings from prior tool calls
///   (used for the "Previously executed" summary AND the read-only check)
/// - `all_results`: `(tool_name, result_text)` accumulator across rounds
pub(crate) fn build_list_only_dead_end_prompt(
    user_message: &str,
    executed_commands: &[&str],
    all_results: &[(String, String)],
) -> Option<String> {
    if !user_message_requires_action(user_message) {
        return None;
    }
    if !all_tools_were_read_only(executed_commands, all_results) {
        return None;
    }

    let action_hint = extract_action_hint(user_message);

    // If we can't name the action, the verb match was almost certainly a
    // false positive — e.g. 「温度趋势的数据绑定好像不对?」 matches the
    // verb 绑定 as part of the NOUN 数据绑定 (data binding). Injecting an
    // unnamed "execute the action NOW" demand contradicts the model every
    // round and burns the whole iteration budget without producing text
    // (observed: 11 rounds / 17 tool calls / no final answer).
    if action_hint.is_empty() {
        tracing::debug!(
            "List-only dead end suspected, but no action hint could be extracted \
             (likely a noun false-positive on an action verb) — skipping forced continuation"
        );
        return None;
    }

    tracing::warn!(
        "List-only dead end detected! User wants action '{}' but only list/query tools were called. Injecting forced continuation.",
        action_hint
    );

    let executed_summary = if executed_commands.is_empty() {
        String::from("(no prior commands)")
    } else {
        executed_commands
            .iter()
            .map(|s| format!("- {}", s))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let mut msg = format!(
        "⚠️ CRITICAL: The user asked you to perform an ACTION, but you ONLY ran list/query commands.\n\
        You MUST now execute the actual action via the `shell` tool running the `neomind` CLI — these platform domains (rule/agent/device/dashboard/widget/extension/message) are CLI-managed; web_fetch is never the way for them.\n\n\
        Previously executed (read-only):\n{}\n\n",
        executed_summary
    );

    if !action_hint.is_empty() {
        msg.push_str(&format!(
            "The user's original request requires: {}\n\
            You MUST output a tool call NOW to complete this action.\n\
            Use the IDs/data from the list results above to construct the command.\n\n",
            action_hint
        ));

        // Widget-to-dashboard tasks have a TWO-STEP pipeline the generic
        // "execute the action" demand glossed over: the widget FILES must be
        // REGISTERED (`widget install`) before add-components accepts the
        // type — and --components takes full component OBJECTS, never widget
        // names (live: four runs guessed string arrays and rebuilt widget
        // files instead of fixing the command).
        if action_hint.contains("dashboard") && action_hint.contains("add") {
            msg.push_str(
                "⚠️ WIDGET → DASHBOARD PIPELINE — do these IN ORDER:\n\
                1) If you created widget FILES this turn: `neomind widget install <dir>` — this registers the widget TYPE; add-components rejects unregistered types.\n\
                2) `neomind dashboard add-components <ID> --components '[{\"id\":\"c1\",\"type\":\"<widget-type>\",\"position\":{\"x\":0,\"y\":0,\"w\":4,\"h\":3}}]' — AFTER install, <widget-type> IS the widget's id (e.g. \"drawing-component\"), NOT the manifest's category field.\n\
                The --components value is an ARRAY OF FULL COMPONENT OBJECTS (id + type + position) — never widget names or ids.\n\
                Component ids and free grid positions come from `neomind dashboard get <ID>`.\n\n",
            );
        }

        // Rule-specific: if creating a rule, verify device/metric discovery was done
        if action_hint.contains("rule") && action_hint.contains("create") {
            let has_device_list = executed_commands
                .iter()
                .any(|c| c.contains("device list") || c.contains("device get"));
            if !has_device_list {
                msg.push_str(
                    "⚠️ RULE CREATION REQUIRES METRIC DISCOVERY:\n\
                    You have NOT run `neomind device list` or `neomind device get <ID>` yet.\n\
                    You CANNOT create a rule without knowing the REAL device ID and metric field name.\n\
                    Run `neomind device list` FIRST to discover metric_fields, THEN construct the DSL.\n\
                    NEVER guess device IDs or metric names — they will silently fail.\n\n"
                );
            }
        }
    }

    msg.push_str(
        "DO NOT output text. DO NOT summarize the list results. DO NOT say 'I found the ...'.\n\
        OUTPUT A TOOL CALL JSON ARRAY NOW to execute the action.",
    );
    Some(msg)
}

/// TURN-END completeness note: the user asked for an ACTION and no
/// successful mutation DELIVERED it this turn. Covers the failure shapes
/// observed live: the action never ran at all (files written,
/// install/add-components skipped), every attempt failed (add-components
/// exit 1 ×2), AND the round-7 shape where a DIFFERENT mutation succeeded
/// (widget create exit 0 while the requested add-to-dashboard never ran —
/// "any mutation succeeded" suppressed the note and the turn ended
/// promising "let me install it now"). Delivery is judged by
/// `mutation_satisfies_action` against the same action vocabulary as the
/// hint. Scoping matches the mid-turn detector: an action hint must be
/// extractable (noun false-positives skipped); with no extractable hint we
/// stay silent.
pub(crate) fn build_incomplete_action_note(
    user_message: &str,
    executed_commands: &[&str],
    successful_mutations: &[String],
) -> Option<String> {
    if !user_message_requires_action(user_message) {
        return None;
    }
    let (domain, action) = extract_action_parts(user_message);
    if domain.is_empty() && action.is_empty() {
        return None;
    }
    let delivered = successful_mutations
        .iter()
        .any(|c| mutation_satisfies_action(c, domain, action));
    if delivered {
        return None;
    }
    let action_hint = extract_action_hint(user_message);
    let done_summary = if executed_commands.is_empty() {
        "no commands ran".to_string()
    } else {
        format!(
            "commands ran but none of them changed anything: {}",
            executed_commands.join("; ")
        )
    };
    // User-facing guard notes are English only (decision 2026-10-10: one
    // fixed language beats per-message detection — locale-independent and
    // matches the UI's English system-callout styling), with no header
    // markup: the frontend renders them as a distinct system callout, not
    // as part of the reply text.
    Some(format!(
        "The requested action ({action_hint}) was NOT successfully executed this turn ({done_summary}). The reply above is a progress report, not a result. Send \"continue\" to resume — fix the command per the reported error first, and take identifiers from list/get output."
    ))
}

/// Extract the domain and expected action from the user message for the forced prompt.
pub(crate) fn extract_action_hint(msg: &str) -> String {
    let (domain, action) = extract_action_parts(msg);
    if domain.is_empty() && action.is_empty() {
        String::new()
    } else if domain.is_empty() {
        format!("the {} action", action)
    } else if action.is_empty() {
        format!("neomind {}", domain)
    } else {
        // Name the REAL delivering subcommand, not the class label: the
        // compat filter accepted `add-components` as delivering "add", and
        // a hint that reads like a command must BE one ("neomind dashboard
        // add" / "neomind message create" name subcommands that do not
        // exist; their real deliveries are add-components/channel-create).
        let real = delivering_subcommand(domain, action).unwrap_or(action);
        format!("neomind {} {}", domain, real)
    }
}

/// Action classes in the shared hint/mutation vocabulary. One table, two
/// consumers: `extract_action_parts` (what did the user ask for) and
/// `mutation_satisfies_action` (did a successful command deliver it).
///
/// - message keywords: the old first-match chain verbatim, in the same
///   order — class priority IS table position;
/// - subcommand tokens: a CLI subcommand DELIVERS the class when any
///   hyphen-token of its name equals one of these (`add-components` → add,
///   `write-metric` → write/send, `channel-create` → create).
const ACTION_CLASSES: &[(&str, &[&str], &[&str])] = &[
    ("create", &["创建", "create", "新建"], &["create"]),
    ("delete", &["删除", "delete", "移除"], &["delete", "remove"]),
    (
        "control",
        &["控制", "control", "打开", "关闭", "开启"],
        &["control"],
    ),
    (
        "enable/start",
        &["启用", "enable", "启动", "start"],
        &["enable", "start"],
    ),
    (
        "disable/stop",
        &["禁用", "disable", "停止", "stop"],
        &["disable", "stop"],
    ),
    ("update", &["更新", "update", "修改", "替换"], &["update"]),
    (
        "write/send",
        &["写入", "write", "发送", "send"],
        &["write", "send"],
    ),
    ("add", &["添加", "add"], &["add"]),
    ("install", &["安装", "install"], &["install"]),
    ("uninstall", &["卸载", "uninstall"], &["uninstall"]),
    ("share", &["分享", "share"], &["share"]),
];

/// The REAL subcommand names of `neomind <domain>`, straight from the live
/// clap tree — the same tree the model drives, so domain/action
/// compatibility can never drift from the binary.
fn cli_domain_subcommands(domain: &str) -> Option<&'static [String]> {
    use std::sync::OnceLock;
    static SURFACES: OnceLock<std::collections::HashMap<String, Vec<String>>> = OnceLock::new();
    let surfaces = SURFACES.get_or_init(|| {
        use clap::CommandFactory;
        fn collect(cmd: &clap::Command, out: &mut Vec<String>) {
            for sub in cmd.get_subcommands() {
                if sub.get_name() == "help" {
                    continue;
                }
                out.push(sub.get_name().to_string());
                collect(sub, out);
            }
        }
        let root = neomind_cli_ops::dispatch::commands::Args::command();
        let mut map = std::collections::HashMap::new();
        for domain in root.get_subcommands() {
            let mut names = Vec::new();
            collect(domain, &mut names);
            map.insert(domain.get_name().to_string(), names);
        }
        map
    });
    surfaces.get(domain).map(|v| v.as_slice())
}

/// The first (tree-order) `neomind <domain>` subcommand that DELIVERS
/// `class`, if any. `None` means the domain has no delivering subcommand —
/// the class is undeliverable there and a hint must not name it.
fn delivering_subcommand(domain: &str, class: &str) -> Option<&'static str> {
    let subs = cli_domain_subcommands(domain)?;
    let (_, _, tokens) = ACTION_CLASSES.iter().find(|(c, _, _)| *c == class)?;
    subs.iter()
        .find(|s| s.split('-').any(|part| tokens.contains(&part)))
        .map(|s| s.as_str())
}

/// Domain + action class the user asked for. Identical to the old
/// first-match chain, plus one refinement: when the domain is known, a
/// candidate class the domain's real CLI surface cannot deliver is skipped
/// in favor of the next match. A hint naming a nonexistent subcommand
/// actively steers the model wrong (live 2026-10-09: a timer SPEC's 停止
/// — the widget's own stop control — outranked the actual 添加, the
/// dead-end prompt pushed "neomind dashboard disable/stop", and the
/// add-components pipeline guide keyed on "add" never fired).
/// `extract_action_hint` additionally resolves the class to the REAL
/// delivering subcommand name (`add` → `add-components`), so a hint that
/// reads like a command always IS one.
pub(crate) fn extract_action_parts(msg: &str) -> (&'static str, &'static str) {
    let msg_lower = msg.to_lowercase();

    // Domain detection
    let domain = if msg_lower.contains("规则") || msg_lower.contains("rule") {
        "rule"
    } else if msg_lower.contains("agent")
        || msg_lower.contains("代理")
        || msg_lower.contains("智能体")
    {
        "agent"
    } else if msg_lower.contains("设备")
        || msg_lower.contains("device")
        || msg_lower.contains("sensor")
    {
        "device"
    } else if msg_lower.contains("仪表盘")
        || msg_lower.contains("仪表板")
        || msg_lower.contains("dashboard")
        || msg_lower.contains("面板")
    {
        "dashboard"
    } else if msg_lower.contains("转换") || msg_lower.contains("transform") {
        "transform"
    } else if msg_lower.contains("组件")
        || msg_lower.contains("widget")
        || msg_lower.contains("小部件")
    {
        "widget"
    } else if msg_lower.contains("扩展")
        || msg_lower.contains("extension")
        || msg_lower.contains("插件")
    {
        "extension"
    } else if msg_lower.contains("消息")
        || msg_lower.contains("message")
        || msg_lower.contains("通知")
        || msg_lower.contains("通道")
        || msg_lower.contains("channel")
    {
        "message"
    } else {
        ""
    };

    // Action candidates in table-priority order (= the old chain order).
    let candidates: Vec<&str> = ACTION_CLASSES
        .iter()
        .filter(|(_, kws, _)| kws.iter().any(|k| msg_lower.contains(k)))
        .map(|(class, _, _)| *class)
        .collect();

    let action = if domain.is_empty() || candidates.is_empty() {
        candidates.first().copied().unwrap_or("")
    } else {
        candidates
            .iter()
            .copied()
            .find(|class| delivering_subcommand(domain, class).is_some())
            // No candidate is a real subcommand of this domain → keep the
            // domain, drop the action: an unnamed "do it" beats a named
            // but fake subcommand.
            .unwrap_or("")
    };

    (domain, action)
}

/// Mutation command patterns (shared): a shell command containing any of
/// these mutates platform state. Used by the read-only detector AND the
/// turn-end "did any mutation SUCCEED" tracker. Audited against the full
/// live CLI surface: every state-changing subcommand must appear here or
/// its successes go untracked (market-install, push start/stop, llm
/// activate, draft approve/reject, …) — and read-only ones must NOT
/// (`push list` used to match the bare " push" shell pattern, killing
/// dead-end detection for the whole push domain).
pub(crate) const MUTATION_COMMAND_PATTERNS: &[&str] = &[
    " create",
    " delete",
    " update",
    " control",
    " enable",
    " disable",
    " write-metric",
    " send-message",
    " share",
    " install ",
    " uninstall ",
    " control ",
    " send ",
    " channel-create",
    " channel-update",
    " channel-delete",
    " add-component",
    " remove-component",
    " update-component",
    // Market installs register a component — bare " install " misses them
    // (hyphen prefix). Bounded: market-install takes a required id.
    " market-install ",
    // Lifecycle switches; bounded with a trailing space so filter values
    // ("--status starting", "--filter activated") don't match.
    " start ",
    " stop ",
    " activate ",
    // State changes with no action class of their own (generic): broker
    // subscriptions, agent runs, memory wipes, onboarding draft decisions,
    // extension lifecycle, config setters, retention.
    " subscribe ",
    " unsubscribe ",
    " invoke ",
    " clear-memory ",
    " approve ",
    " reject ",
    " reload ",
    " build ",
    " --set",
    " set-retention",
    // Generic shell-side mutations. NOTE "git push", not " push": the bare
    // form matched the read-only `neomind push list` domain commands.
    " publish",
    "git push",
    " register",
    " import",
];

/// Does this shell command look like a mutation?
pub(crate) fn is_mutation_command(cmd: &str) -> bool {
    MUTATION_COMMAND_PATTERNS.iter().any(|p| cmd.contains(p))
}

/// Mutation-pattern → action class (the same vocabulary ACTION_CLASSES
/// uses for hints). Order matters: " uninstall " must classify before
/// " install " matches inside it. Patterns with no class
/// (publish/push/register/import) stay generic.
fn pattern_action_class(pattern: &str) -> Option<&'static str> {
    Some(match pattern {
        p if p.contains("uninstall") => "uninstall",
        p if p.contains("install") => "install",
        p if p.contains("create") => "create",
        p if p.contains("delete") || p.contains("remove") => "delete",
        p if p.contains("update") => "update",
        p if p.contains("control") => "control",
        p if p.contains("enable") => "enable/start",
        p if p.contains("disable") => "disable/stop",
        p if p.contains("write") || p.contains("send") => "write/send",
        p if p.contains("add") => "add",
        p if p.contains("share") => "share",
        _ => return None,
    })
}

/// The `neomind <domain>` token of a command, when present.
fn command_domain(cmd: &str) -> Option<String> {
    let mut toks = cmd.split_whitespace();
    let first = toks.next()?;
    if !first.ends_with("neomind") {
        return None;
    }
    toks.next().map(str::to_lowercase)
}

/// Did this SUCCESSFUL mutation command DELIVER the requested
/// (domain, action)? Relevance, not bare occurrence — the round-7 live
/// incident: `widget create` succeeded, the requested add-to-dashboard
/// never ran, and "any mutation succeeded" silenced the completeness note
/// while the model ended the turn promising "let me install it now".
///
/// Rules: a segment can only deliver via a mutation it actually performs;
/// when both the hint and the segment name a domain they must agree; when
/// an action class was extracted from the request, the segment's mutation
/// class must match it. Mutations with no class (publish/import/…) keep the
/// old conservative satisfied-by-default semantics rather than inventing
/// precision. Compound commands (`a && b`, `a; b`) deliver when ANY
/// segment does — judging only the first would flag a delivered
/// add-components chained after an install, while non-mutation noise
/// segments (`; echo done`) must NOT deliver anything.
pub(crate) fn mutation_satisfies_action(cmd: &str, hint_domain: &str, hint_action: &str) -> bool {
    cmd.split("&&")
        .flat_map(|seg| seg.split(';'))
        .any(|seg| segment_satisfies_action(seg, hint_domain, hint_action))
}

fn segment_satisfies_action(seg: &str, hint_domain: &str, hint_action: &str) -> bool {
    let seg_lower = seg.to_lowercase();

    let matched: Vec<&str> = MUTATION_COMMAND_PATTERNS
        .iter()
        .copied()
        .filter(|p| seg_lower.contains(p))
        .collect();
    // Delivery requires a mutation; a noise segment (`echo done`) inside a
    // compound command must not satisfy anything — without this gate the
    // unclassified-mutation fallback below let ANY trailing segment count.
    if matched.is_empty() {
        return false;
    }

    if !hint_domain.is_empty() {
        if let Some(d) = command_domain(&seg_lower) {
            if d != hint_domain {
                return false;
            }
        }
    }

    if hint_action.is_empty() {
        return true;
    }
    let classes: Vec<&str> = matched
        .iter()
        .filter_map(|p| pattern_action_class(p))
        .collect();
    classes.is_empty() || classes.contains(&hint_action)
}

// ---------------------------------------------------------------------------
// Stated-intent mismatch (promise-action coherence)
// ---------------------------------------------------------------------------

/// `neomind <domain> <sub>` commands the narration NAMES — the model's own
/// stated intent. Only real domain/subcommand pairs count (checked against
/// the live clap tree), so prose like "the neomind CLI" never registers.
pub(crate) fn promised_commands(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let toks: Vec<&str> = lower.split_whitespace().collect();
    let mut out: Vec<String> = Vec::new();
    for w in toks.windows(3) {
        if !w[0].ends_with("neomind") {
            continue;
        }
        let d = w[1].trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-');
        let s = w[2].trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-');
        if let Some(subs) = cli_domain_subcommands(d) {
            if subs.iter().any(|x| x == s) {
                out.push(format!("neomind {d} {s}"));
            }
        }
    }
    out
}

/// One round of the stated-intent tracker (caller keeps the state across
/// rounds). `narration` is the round's visible text; `executed_shell_cmds`
/// every shell command the round ATTEMPTED (success or failure — an
/// attempt is not an unkept promise).
///
/// Live shape this tracks (2026-10-10 tomato-clock session, rounds 25-37):
/// every reply ended "Let me use the neomind dashboard add-components
/// command" — never executed, while file_write/skill load ran instead.
/// Every existing guard passed: tools WERE called, they were not all
/// read-only, and narration wasn't tool-less. The signal is REPETITION of
/// a concrete promise without execution, not a single mention (a strong
/// model legitimately stating intent once before prep work must not be
/// nagged).
pub(crate) fn promise_tracker_step(
    narration: &str,
    executed_shell_cmds: &[String],
    tracked: &mut Option<String>,
    rounds: &mut usize,
) {
    // Execution resets: once the promised command runs (even a failed
    // attempt), the promise is kept as far as this guard is concerned.
    if let Some(cmd) = tracked {
        let done = executed_shell_cmds.iter().any(|c| {
            let cl = c.to_lowercase();
            cl.starts_with(cmd.as_str()) || cl.contains(cmd.as_str())
        });
        if done {
            *tracked = None;
            *rounds = 0;
            return;
        }
    }

    if let Some(last) = promised_commands(narration).last() {
        if tracked.as_deref() == Some(last.as_str()) {
            *rounds += 1;
        } else {
            *tracked = Some(last.clone());
            *rounds = 1;
        }
    }
}

/// The mid-turn directive for a repeatedly promised, never executed
/// command. Advisory in tone and actionable: re-run the exact command the
/// model itself stated, or name what is missing — never a bare "do it".
pub(crate) fn build_unkept_promise_directive(cmd: &str, rounds: usize) -> String {
    format!(
        "⚠️ STATED-INTENT MISMATCH: your last {rounds} replies each said you would run \
         `{cmd}` — but it was never executed, while other tools ran instead. Either run \
         `{cmd}` NOW with the `shell` tool (re-use the exact full command you already \
         stated, including its identifiers), or reply with ONE sentence naming the \
         identifier/information that is missing. Do not restate the plan and do not \
         run other tools first.\n\
         你的最近 {rounds} 轮回复都说明要执行 `{cmd}`，但它从未被执行。现在就用 shell 工具执行它\
         （直接复用你陈述过的完整命令），或用一句话说明缺少哪个标识/信息。不要再次复述计划。"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_message_requires_action_chinese() {
        assert!(user_message_requires_action("创建新设备"));
        assert!(user_message_requires_action("删除旧配置"));
        assert!(user_message_requires_action("启用监控"));
        assert!(user_message_requires_action("发送消息"));
        assert!(!user_message_requires_action("列出所有设备"));
        assert!(!user_message_requires_action("查看状态"));
    }

    #[test]
    fn test_user_message_requires_action_english() {
        assert!(user_message_requires_action("create device"));
        assert!(user_message_requires_action("delete old config"));
        assert!(user_message_requires_action("STOP the service"));
        assert!(!user_message_requires_action("list devices"));
        assert!(!user_message_requires_action("get status"));
    }

    #[test]
    fn test_all_tools_were_read_only() {
        assert!(all_tools_were_read_only(
            &["neomind device list", "neomind rule list"],
            &[]
        ));
        assert!(!all_tools_were_read_only(
            &["neomind device create sensor1"],
            &[]
        ));
        assert!(!all_tools_were_read_only(
            &["neomind device list", "neomind device delete sensor1"],
            &[]
        ));
        assert!(!all_tools_were_read_only(&[], &[])); // empty = not read-only
    }

    #[test]
    fn test_extract_action_hint() {
        assert_eq!(extract_action_hint("创建新规则"), "neomind rule create");
        assert_eq!(extract_action_hint("删除设备"), "neomind device delete");
        assert_eq!(
            extract_action_hint("创建仪表盘"),
            "neomind dashboard create"
        );
        assert_eq!(extract_action_hint("查看状态"), "");
    }

    #[test]
    fn test_noun_verb_collision_skips_forced_continuation() {
        // Real incident (2026-08-22): 「温度趋势的数据绑定好像不对?」— the
        // noun 数据绑定 contains the ACTION_VERB 绑定, so requires_action
        // matched, but extract_action_hint found no action. The injected
        // "execute the action NOW" prompt then contradicted the model every
        // round → 11 rounds / 17 tool calls / no final text.
        let msg = "温度趋势的数据绑定好像不对?";
        assert!(
            user_message_requires_action(msg),
            "verb collision still matches"
        );
        assert_eq!(extract_action_hint(msg), "");

        // Read-only investigation commands → dead-end condition holds, but
        // the empty hint must suppress the injection.
        let cmds = [
            "neomind widget get sparkline",
            "neomind device get demo-001",
        ];
        assert!(all_tools_were_read_only(&cmds, &[]));
        assert!(build_list_only_dead_end_prompt(msg, &cmds, &[]).is_none());
    }

    #[test]
    fn test_named_action_still_injects() {
        // Guard must not over-suppress: a genuine action request with a
        // nameable action still injects.
        let msg = "创建一个温湿度仪表盘";
        let cmds = ["neomind device list", "neomind widget list"];
        assert!(build_list_only_dead_end_prompt(msg, &cmds, &[]).is_some());
    }

    /// Turn-end completeness: fires when an action was requested and only
    /// read-only commands ran; stays silent for mutations, non-action asks,
    /// and the no-shell-evidence case.
    #[test]
    fn incomplete_action_note_fires_and_scopes() {
        // The live incident: "添加到dashboard", only --help/list shell calls.
        // Notes are English only regardless of the user's message language
        // (decision 2026-10-10: one fixed, locale-independent language).
        let note = build_incomplete_action_note(
            "帮我开发一个前端画图组件 并且添加到dashboard",
            &[
                "neomind dashboard list --help 2>&1 | head -50",
                "neomind dashboard list --help 2>&1 | head -50",
            ],
            &[],
        )
        .expect("must flag the un-executed action");
        assert!(note.contains("NOT successfully executed"), "{note}");
        assert!(note.contains("add"), "action hint carried: {note}");
        assert!(!note.contains("系统提示"), "no Chinese header: {note}");
        // English user message → the SAME fixed English note.
        let en = build_incomplete_action_note(
            "add it to my dashboard",
            &["neomind dashboard list"],
            &[],
        )
        .expect("must flag");
        assert!(en.contains("NOT successfully executed"), "{en}");
        assert!(!en.contains("未"), "fixed language, no CJK: {en}");
        // The REQUESTED mutation SUCCEEDED → delivered; no note.
        assert!(build_incomplete_action_note(
            "帮我加到dashboard",
            &["neomind dashboard add-components d1 --components '[{...}]'"],
            &["neomind dashboard add-components d1 --components '[{...}]'".to_string()],
        )
        .is_none());
        // Pure query → no note (no extractable action hint).
        assert!(
            build_incomplete_action_note("现在有哪些设备?", &["neomind device list"], &[])
                .is_none()
        );
        // No shell activity at all BUT an action was requested (e.g. a
        // widget built purely via file_write, never installed) → NOTE
        // (supersedes the old silence-on-undeterminable rule, which missed
        // exactly this live case).
        let no_shell = build_incomplete_action_note("帮我创建规则", &[], &[])
            .expect("action requested with zero successful mutations must be flagged");
        assert!(no_shell.contains("no commands ran"), "{no_shell}");
    }

    /// Live incident (2026-10-09 round 7): a timer SPEC mentioning 停止 —
    /// the widget's own stop control — won the action match over the actual
    /// 添加, producing the nonexistent "neomind dashboard disable/stop"
    /// while skipping the add-components pipeline guide keyed on "add".
    /// Hints also name the REAL delivering subcommand, never a class label
    /// that reads like a command but isn't one.
    #[test]
    fn action_hint_prefers_subcommand_the_domain_actually_has() {
        let msg = "开发一个番茄时钟组件并添加到dashboard中展示，支持启动和停止";
        assert_eq!(extract_action_hint(msg), "neomind dashboard add-components");
        // A domain that really has the subcommand keeps it — under its real
        // name ("disable", not the class label "disable/stop").
        assert_eq!(
            extract_action_hint("帮我停止这条规则"),
            "neomind rule disable"
        );
        // Compound deliveries resolve to the delivering subcommand too.
        assert_eq!(
            extract_action_hint("帮我创建通知通道"),
            "neomind message channel-create"
        );
        // No compatible action on the domain → domain-only hint, never a
        // fake subcommand.
        assert_eq!(extract_action_hint("禁用这个仪表盘"), "neomind dashboard");
        // No domain in the message → old first-match behavior.
        assert_eq!(extract_action_hint("帮我停止"), "the disable/stop action");
    }

    #[test]
    fn mutation_relevance_not_occurrence() {
        // Scaffold success on ANOTHER domain does not deliver a dashboard add.
        assert!(!mutation_satisfies_action(
            "neomind widget create \"番茄时钟\" --widget-type gauge",
            "dashboard",
            "add",
        ));
        // The real delivery.
        assert!(mutation_satisfies_action(
            "neomind dashboard add-components d1 --components '[{...}]'",
            "dashboard",
            "add",
        ));
        // Same domain, wrong class: a dashboard create is not an add.
        assert!(!mutation_satisfies_action(
            "neomind dashboard create x",
            "dashboard",
            "add",
        ));
        // Generic hint: class match is enough.
        assert!(mutation_satisfies_action(
            "neomind rule create r1",
            "",
            "create"
        ));
        // Domain-only hint: any mutation on the domain counts.
        assert!(mutation_satisfies_action(
            "neomind rule create r1",
            "rule",
            ""
        ));
        // Unclassifiable mutation patterns keep the old any-mutation rule
        // (same domain, no action class to compare).
        assert!(mutation_satisfies_action(
            "neomind rule import f.csv",
            "rule",
            "create"
        ));
        // Compound commands deliver when ANY segment does.
        assert!(mutation_satisfies_action(
            "neomind widget install gauge-3832 && neomind dashboard add-components d1 --components '[]'",
            "dashboard",
            "add",
        ));
        // Noise segments must NOT deliver — the unclassified-mutation
        // fallback used to let any trailing segment satisfy (adversarial
        // review: `widget create x; echo done` would have silenced the
        // note for a pending dashboard add).
        assert!(!mutation_satisfies_action(
            "neomind widget create x; echo done",
            "dashboard",
            "add",
        ));
        // A semicolon inside argument DATA must not fake-deliver either.
        assert!(!mutation_satisfies_action(
            "neomind widget create x --description \"a;b\"",
            "dashboard",
            "add",
        ));
    }

    /// Round-7 shape: `widget create` succeeded, the requested
    /// add-to-dashboard never ran — the note MUST fire (the old
    /// any-mutation predicate stayed silent) and MUST stay silent once the
    /// dashboard mutation itself succeeds.
    #[test]
    fn incomplete_note_demands_the_requested_mutation() {
        let msg = "开发一个番茄时钟组件并添加到dashboard中展示，支持启动和停止";
        let note = build_incomplete_action_note(
            msg,
            &[
                "neomind widget create \"番茄时钟\" --widget-type gauge",
                "neomind widget install gauge-3832",
            ],
            &["neomind widget create \"番茄时钟\" --widget-type gauge".to_string()],
        )
        .expect("a widget-domain create must NOT satisfy a dashboard add");
        assert!(note.contains("add"), "{note}");
        assert!(build_incomplete_action_note(
            msg,
            &["neomind dashboard add-components d1 --components '[]'"],
            &["neomind dashboard add-components d1 --components '[]'".to_string()],
        )
        .is_none());
    }

    /// Widget-to-dashboard tasks must get the two-step pipeline (install →
    /// add-components with component OBJECTS) — four live runs guessed
    /// string arrays instead.
    #[test]
    fn dead_end_injection_carries_widget_pipeline() {
        let msg = build_list_only_dead_end_prompt(
            "帮我开发一个前端画图组件 并且添加到dashboard",
            &["neomind widget list"],
            &[],
        )
        .expect("action request must inject");
        assert!(msg.contains("widget install"), "{msg}");
        assert!(msg.contains("ARRAY OF FULL COMPONENT OBJECTS"), "{msg}");
        assert!(msg.contains("--components"), "{msg}");
    }

    /// The dead-end injection must NAME the shell tool — the live incident
    /// had a small model answer "output a tool call NOW" with web_fetch.
    #[test]
    fn dead_end_injection_names_shell_tool() {
        let msg = build_list_only_dead_end_prompt(
            "帮我开发一个前端画图组件 并且添加到dashboard",
            &["neomind dashboard list", "neomind widget list"],
            &[],
        )
        .expect("action request with only list calls must inject");
        assert!(msg.contains("`shell` tool"), "{msg}");
        assert!(msg.contains("web_fetch is never the way"), "{msg}");
    }

    #[test]
    fn test_component_mutations_are_not_read_only() {
        // Incident addendum: an add-components + remove-components pair IS a
        // mutation — the detector used to keep firing after the change was
        // already applied, burning the turn to the 5-minute wall clock.
        assert!(!all_tools_were_read_only(
            &["neomind dashboard add-components d1 --components '[]'"],
            &[]
        ));
        assert!(!all_tools_were_read_only(
            &["neomind dashboard remove-components d1 --ids '[\"c3\"]'"],
            &[]
        ));
        assert!(!all_tools_were_read_only(
            &["neomind dashboard update-component d1 --id c3 --set '{}'"],
            &[]
        ));
        // Query commands remain read-only.
        assert!(all_tools_were_read_only(
            &["neomind dashboard get d1", "neomind widget list"],
            &[]
        ));
    }

    /// The mutation-pattern audit (against the full live CLI surface):
    /// every state-changing subcommand must register as a mutation, and
    /// read-only commands must not — the bare " push" shell pattern used
    /// to match `neomind push list`, killing dead-end detection for the
    /// whole push domain, while market-install / push start / llm
    /// activate / draft approve successes went entirely untracked.
    #[test]
    fn mutation_patterns_cover_the_live_surface() {
        // Mutations the old table missed.
        for cmd in [
            "neomind widget market-install gauge-chart",
            "neomind extension market-install laya-decision",
            "neomind push start webhook-1",
            "neomind push stop webhook-1",
            "neomind llm activate ollama-local",
            "neomind connector subscribe device/+/status",
            "neomind connector unsubscribe device/+/status",
            "neomind agent invoke inspect-agent",
            "neomind agent clear-memory demo-agent",
            "neomind device drafts approve ne101-001",
            "neomind device drafts reject ne101-001",
            "neomind extension reload laya-decision",
            "neomind extension build ./my-ext/",
            "neomind extension config laya-decision --set '{\"k\":1}'",
            "neomind settings set-retention --enabled true --days 30",
        ] {
            assert!(is_mutation_command(cmd), "must be a mutation: {cmd}");
            assert!(!all_tools_were_read_only(&[cmd], &[]), "{cmd}");
        }
        // Read-only shapes that must NOT match (bounded/substring traps).
        for cmd in [
            "neomind push list",
            "neomind push get webhook-1",
            "neomind push stats",
            "neomind connector subscriptions",
            "neomind agent list --status starting",
            "neomind llm list",
            "neomind device drafts list",
        ] {
            assert!(!is_mutation_command(cmd), "false mutation: {cmd}");
            assert!(all_tools_were_read_only(&[cmd], &[]), "{cmd}");
        }
        // New lifecycle patterns classify into the right action classes so
        // delivery matching works (push start delivers an enable/start).
        assert!(mutation_satisfies_action(
            "neomind push start webhook-1",
            "",
            "enable/start"
        ));
        assert!(mutation_satisfies_action(
            "neomind widget market-install gauge-chart",
            "widget",
            "install"
        ));
    }

    /// The stated-intent mismatch guard: repetition of a concrete command
    /// promise without execution is the signal — a single mention before
    /// legitimate prep work must NOT trigger (strong-model friendly).
    #[test]
    fn promise_tracker_fires_on_repetition_only() {
        // The live round-8 narration, verbatim shape.
        let narration = "I need to add the tomato-clock widget to the temperature/humidity \
                         dashboard (ID: dashboard_63df). Let me use the neomind dashboard \
                         add-components command.";
        let files = ["neomind widget list".to_string()]; // tools ran, not the promise

        let mut tracked: Option<String> = None;
        let mut rounds = 0usize;

        // Round 1: first mention — track, no trigger.
        promise_tracker_step(narration, &files, &mut tracked, &mut rounds);
        assert_eq!(rounds, 1, "first mention records but does not fire");
        assert_eq!(tracked.as_deref(), Some("neomind dashboard add-components"));

        // Round 2: same promise, still not executed → threshold reached.
        promise_tracker_step(narration, &files, &mut tracked, &mut rounds);
        assert_eq!(rounds, 2);

        // Execution (even a FAILED attempt) resets the tracker.
        let attempted = [
            "neomind widget list".to_string(),
            "neomind dashboard add-components dashboard_63df --components '[{...}]'".to_string(),
        ];
        promise_tracker_step(narration, &attempted, &mut tracked, &mut rounds);
        assert_eq!(rounds, 0, "an attempt is not an unkept promise");
        assert!(tracked.is_none());

        // A single mention followed by OTHER narration (no re-promise)
        // never accumulates — the strong-model prep-work case.
        let mut tracked: Option<String> = None;
        let mut rounds = 0usize;
        promise_tracker_step(narration, &files, &mut tracked, &mut rounds);
        promise_tracker_step(
            "Reading the manifest to confirm global_name.",
            &files,
            &mut tracked,
            &mut rounds,
        );
        assert_eq!(rounds, 1, "non-promising rounds do not accumulate");

        // Prose mentions of the CLI never register as promises.
        assert!(promised_commands("Run the neomind CLI to manage your platform.").is_empty());
        assert_eq!(
            promised_commands("use the neomind widget install frontend-components/x step"),
            vec!["neomind widget install".to_string()]
        );
        let directive = build_unkept_promise_directive("neomind dashboard add-components", 2);
        assert!(directive.contains("STATED-INTENT MISMATCH"), "{directive}");
        assert!(directive.contains("add-components"), "{directive}");
    }
}
