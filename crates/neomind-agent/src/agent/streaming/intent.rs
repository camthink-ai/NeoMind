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
    "接入",
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
    "connect",
    "onboard",
];

/// Instructional-question shapes ("如何创建设备？", "how do I create a
/// device?"). These CONTAIN action verbs but ask for an explanation — no
/// action is being requested, so the turn-end completeness note must not
/// label a correct explanatory answer as "the requested action was NOT
/// executed", and the list-only coercion must not demand a phantom command.
/// Same over-broad-pattern family as the 2026-10-20 narration incident.
/// Deliberately narrow: genuine requests phrased as questions ("你能帮我
/// 创建吗？", "can you create…") do NOT match and still trigger the guards.
const INSTRUCTIONAL_QUESTION_PATTERNS: &[&str] = &[
    "如何",
    "怎么",
    "怎样",
    "为什么",
    "什么是",
    "是什么",
    "如何才能",
    "how do i",
    "how to",
    "how can i",
    "how does",
    "what is",
    "what's",
    "explain",
];

/// True when `c` is an ASCII word constituent (letter/digit/underscore).
fn is_ascii_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Find all occurrences of `needle` in `haystack`, enforcing ASCII word
/// boundaries when `needle` is ASCII (so `add` never hits `address`/`padding`,
/// `start` never hits `startup`/`restart`). CJK needles collect every substring
/// occurrence so an excluded first mention ("已安装") never hides a genuine
/// write verb later in the sentence ("并安装weather").
fn find_all_action_verb_positions(haystack: &str, needle: &str) -> Vec<usize> {
    let mut positions = Vec::new();
    if needle.is_empty() {
        return positions;
    }
    let ascii_needle = needle.is_ascii();
    let step = needle.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    let mut search_from = 0usize;
    while search_from < haystack.len() {
        let Some(rel) = haystack[search_from..].find(needle) else {
            break;
        };
        let pos = search_from + rel;
        if !ascii_needle {
            positions.push(pos);
            search_from = pos + step;
            continue;
        }
        let before_ok = pos == 0
            || !haystack[..pos]
                .chars()
                .next_back()
                .is_some_and(is_ascii_word_char);
        let after = pos + needle.len();
        let after_ok = after >= haystack.len()
            || !haystack[after..]
                .chars()
                .next()
                .is_some_and(is_ascii_word_char);
        if before_ok && after_ok {
            positions.push(pos);
        }
        search_from = pos + step;
    }
    positions
}

fn find_action_verb(haystack: &str, needle: &str) -> Option<usize> {
    find_all_action_verb_positions(haystack, needle)
        .into_iter()
        .next()
}

/// Chinese completed-aspect / interrogative prefixes that demote a verb to
/// a status description ("已安装", "是否启用", "有没有绑定").
const ZH_COMPLETED_PREFIXES: &[&str] = &[
    "已经",
    "已",
    "曾",
    "是否",
    "有没有",
    "有无",
    "能否",
    "可否",
    "是不是",
];

/// Noun/compound suffixes that turn a verb into a noun phrase
/// ("安装路径", "更新日志", "创建时间", "控制面板").
const ZH_NOUN_SUFFIXES: &[&str] = &[
    "路径", "日志", "时间", "状态", "列表", "链接", "按钮", "面板", "说明", "记录", "历史", "信息",
    "详情", "名称", "方式", "方法",
];

/// Clause punctuation that terminates lookback for Chinese completed-aspect
/// prefixes (so `"已连接，请删除"` does not leak `"已"` across `，`).
fn is_clause_punctuation(c: char) -> bool {
    matches!(c, '，' | '。' | '；' | ',' | ';' | '.' | '\n')
}

/// True when the Chinese verb at `verb_pos` is a completed-aspect /
/// interrogative status mention or a noun-compound, not a write request.
fn chinese_verb_is_non_action(msg: &str, verb: &str, verb_pos: usize) -> bool {
    if verb.chars().any(|c| c.is_ascii()) {
        return false;
    }
    let prefix = &msg[..verb_pos];
    // Stop lookback at the most recent clause boundary punctuation.
    let clause_start = prefix
        .char_indices()
        .rev()
        .find(|(_, c)| is_clause_punctuation(*c))
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    let clause_prefix = &prefix[clause_start..];
    // Look back up to 6 chars within the current clause for aspect/question markers.
    let lookback_start = clause_prefix
        .char_indices()
        .rev()
        .nth(5)
        .map(|(i, _)| i)
        .unwrap_or(0);
    let lookback = &clause_prefix[lookback_start..];
    if ZH_COMPLETED_PREFIXES.iter().any(|p| lookback.contains(p)) {
        return true;
    }
    let after = &msg[verb_pos + verb.len()..];
    ZH_NOUN_SUFFIXES.iter().any(|s| after.starts_with(s))
}

/// True when an action verb at `verb_pos` is under an immediate negation
/// ("不要修改", "don't update", "without changing"). Used so read-only asks
/// that mention a forbidden mutation do not trip turn-end completeness notes
/// (L3 Spark: "只读查询，不要修改配置").
fn action_verb_is_negated(msg_lower: &str, verb_pos: usize) -> bool {
    // Walk back up to 16 Unicode scalars so longer prefaces still count
    // ("不要对任何仪表盘做修改", "don't touch any of the settings").
    let prefix = &msg_lower[..verb_pos];
    let start = prefix
        .char_indices()
        .rev()
        .nth(15)
        .map(|(i, _)| i)
        .unwrap_or(0);
    let window = &prefix[start..];
    window.contains("不要")
        || window.contains("别动")
        || window.contains("别")
        || window.contains("勿")
        || window.contains("无需")
        || window.contains("不用")
        || window.contains("禁止")
        || window.contains("不做任何")
        || window.contains("不做")
        || window.contains("不可")
        || window.contains("不应")
        || window.contains("没有")
        || window.contains("未")
        || window.contains("没")
        || window.contains("只读")
        || window.contains("don't ")
        || window.contains("do not ")
        || window.contains("dont ")
        || window.contains("without ")
        || window.contains("never ")
        || window.contains("no changes")
        || window.contains("read-only")
        || window.contains("readonly")
        || window.contains("don't touch")
        || window.contains("do not touch")
        || window.contains("not to ")
}

/// Check if the user message requests a mutation/action (not just a query).
///
/// Position-sensitive: instructional markers only demote a message from
/// REQUEST to QUESTION when they LEAD the first action verb ("如何创建…",
/// "how do I create…"). The same words AFTER the verb are filler inside a
/// genuine request ("帮我删除这个设备，不知道怎么操作") and must not
/// silence the guard.
///
/// Negation-aware: a message whose ONLY action verbs are negated
/// ("不要修改配置", "don't update anything") is treated as non-action. A
/// mixed request ("创建设备，不要修改配置") still requires action because
/// the un-negated verb remains.
///
/// Morphology-aware (Stage 1): English verbs need word boundaries; Chinese
/// completed-aspect / noun-suffix forms (`已安装`, `安装路径`) are excluded.
pub(crate) fn user_message_requires_action(msg: &str) -> bool {
    let msg_lower = msg.to_lowercase();
    let verb_hits: Vec<(usize, &'static str)> = ACTION_VERBS
        .iter()
        .flat_map(|v| {
            find_all_action_verb_positions(&msg_lower, v)
                .into_iter()
                .map(move |pos| (pos, *v))
        })
        .collect();
    let Some(&(first_verb_pos, _)) = verb_hits.iter().min_by_key(|(pos, _)| *pos) else {
        return false;
    };
    if INSTRUCTIONAL_QUESTION_PATTERNS
        .iter()
        .filter_map(|p| msg_lower.find(p))
        .any(|pos| pos < first_verb_pos)
    {
        return false;
    }
    // Extra interrogative / hypothetical English shapes that often wrap a
    // past-looking verb ("is the extension installed?", "what if I delete…").
    const EXTRA_QUESTION_PREFIXES: &[&str] = &[
        "is the ",
        "is it ",
        "are the ",
        "are there ",
        "did it ",
        "did you ",
        "what if ",
        "what happens if ",
        "would it ",
        "can i safely ",
    ];
    if EXTRA_QUESTION_PREFIXES
        .iter()
        .filter_map(|p| msg_lower.find(p))
        .any(|pos| pos < first_verb_pos)
    {
        return false;
    }
    verb_hits.into_iter().any(|(pos, verb)| {
        !action_verb_is_negated(&msg_lower, pos)
            && !chinese_verb_is_non_action(&msg_lower, verb, pos)
    })
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
        // Capability unify: same derive_tool_capability as write gate / retry.
        let args = serde_json::json!({ "command": cmd });
        let cap = crate::agent::task_continuity::derive_tool_capability("shell", &args);
        if !cap.read_only {
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
/// - `consecutive_read_only_rounds`: PRIOR consecutive all-read-only rounds
///   (the caller counts; the round being decided here is the next one)
pub(crate) fn build_list_only_dead_end_prompt(
    user_message: &str,
    executed_commands: &[&str],
    all_results: &[(String, String)],
    consecutive_read_only_rounds: u32,
) -> Option<String> {
    if !user_message_requires_action(user_message) {
        return None;
    }
    // The user asked for a plan/explanation only — coercing an execution
    // now would contradict the instruction.
    if crate::agent::user_requested_no_execution(user_message) {
        return None;
    }

    // === COERCION EVIDENCE GATE (live 2026-10-19) ===
    // This prompt is a STRONG model-facing directive ("You MUST execute X
    // NOW"), and it fired after ONE exploratory round on a mis-extracted
    // request — demanding a phantom `neomind device create` for 20 minutes
    // while the model was already on the correct path (`device list` +
    // `dashboard list` were the RIGHT first steps of the flow that
    // eventually built the dashboard). A single list-only round is ID
    // discovery, not a dead end: coerce only on the SECOND consecutive
    // all-read-only round. (A separate multi-domain ambiguity gate was
    // considered and rejected: after DOMAIN_KEYWORDS prioritization the
    // extraction picks the most specific artifact domain, and a blanket
    // ambiguity suppressor would also kill the documented widget→dashboard
    // pipeline recoveries, which ride on genuinely multi-domain messages.)
    if consecutive_read_only_rounds < 1 {
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
    // Plan-only request: the reply is not a failed action.
    if crate::agent::user_requested_no_execution(user_message) {
        return None;
    }
    let (domain, action) = extract_action_parts(user_message);
    // Without a recognisable DOMAIN the note cannot tell whether some command
    // in another domain delivered the ask, and "never executed" becomes an
    // overclaim on a successful turn. Live 2026-10-21 (eval): "Send it a
    // set_alarm command" extracted action=write/send + domain empty while a
    // successful `neomind device control fridge-3 set_alarm` (exit 0) ran —
    // the note labelled it a failure. The note is a safety label; an
    // unprovable claim is worse than silence.
    if domain.is_empty() {
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
    // 接入/connect/onboard: guided MQTT onboarding asks (eval
    // guided-mqtt-onboarding) are create-class — without them,
    // skill-exit / incomplete-action stay silent and Spark stalls on
    // skill ↔ list without ever `device create`.
    (
        "create",
        &["创建", "create", "新建", "接入", "connect", "onboard"],
        &["create"],
    ),
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

/// Domain keyword table. Order is DISAMBIGUATION PRIORITY, not mere
/// convention: a request can legitimately name several domains, and the
/// first matching entry wins. `device` is deliberately LAST — on an IoT
/// platform "device" is ambient vocabulary (requests are routinely ABOUT
/// devices: "创建一个展示**设备**图片的**仪表盘**"), so a first-keyword
/// chain that checked 设备 before 仪表盘 extracted `device` from a
/// dashboard request (live 2026-10-19: the dead-end guard then spent the
/// whole turn nagging for a `neomind device create` the user never asked
/// for, and the turn ended with a false "requested ACTION never executed"
/// note AFTER the dashboard had been built). All other pair priorities are
/// the historical chain order, unchanged.
const DOMAIN_KEYWORDS: &[(&str, &[&str])] = &[
    ("rule", &["规则", "rule"]),
    ("agent", &["agent", "代理", "智能体"]),
    ("dashboard", &["仪表盘", "仪表板", "dashboard", "面板"]),
    ("transform", &["转换", "transform"]),
    ("widget", &["组件", "widget", "小部件"]),
    ("extension", &["扩展", "extension", "插件"]),
    ("message", &["消息", "message", "通知", "通道", "channel"]),
    ("device", &["设备", "device", "sensor", "传感器"]),
];

/// Every domain whose keywords occur in the message, in DOMAIN_KEYWORDS
/// priority order. The first entry is what `extract_action_parts` picks;
/// len > 1 documents that the extraction was ambiguous.
pub(crate) fn matched_domains(msg: &str) -> Vec<&'static str> {
    let msg_lower = msg.to_lowercase();
    DOMAIN_KEYWORDS
        .iter()
        .filter(|(_, kws)| kws.iter().any(|k| msg_lower.contains(k)))
        .map(|(d, _)| *d)
        .collect()
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

    // Domain detection: first match in DOMAIN_KEYWORDS priority order —
    // see the table's doc comment for why device ranks last.
    let domain = matched_domains(msg).first().copied().unwrap_or("");

    // Action candidates in table-priority order (= the old chain order).
    let candidates: Vec<&str> = ACTION_CLASSES
        .iter()
        .filter(|(_, kws, _)| {
            kws.iter()
                .any(|k| find_action_verb(&msg_lower, k).is_some())
        })
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
///
/// Detection is case-insensitive; callers that need a write fingerprint must
/// pass the **original** command (arg values keep their case) to
/// [`mutating_write_fingerprint`], not a lowercased copy.
pub(crate) fn is_mutation_command(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    MUTATION_COMMAND_PATTERNS.iter().any(|p| lower.contains(p))
}

/// Canonical business fingerprint for ANY tool call (both read-only and mutating).
///
/// **Single source of truth** across the read-only layer (turn reuse,
/// `ToolResultCache`, duplicate-call advisory, no-progress loop detection) and
/// the write layer (`OperationLedger`, `failed_tool_signatures` circuit breaker).
///
/// Normalizes:
/// - strips `_new_operation` / `_host_prepared_operation_id` / `_prepared_operation_id`
/// - maps CLI-domain aliases (`device`, `rule`, …) to canonical `shell`
/// - folds shell structured `params` (object or array) and CLI `--params` /
///   `--set` / `--components` / `--ids` / `--config` into one sorted-key shape
/// - sorts nested JSON object keys via `stable_json_string`
pub(crate) fn canonical_tool_call_fingerprint(
    tool_name: &str,
    arguments: &serde_json::Value,
) -> String {
    let canonical =
        crate::agent::task_continuity::canonicalize_tool_arguments(tool_name, arguments);
    let real = crate::tools::resolve_tool_name(tool_name);
    canonical_tool_call_fingerprint_from_canonical(real.as_str(), &canonical)
}

/// Fingerprint for the per-turn consecutive-failure circuit breaker.
///
/// Same as [`canonical_tool_call_fingerprint`] except shell
/// `neomind transform create|update` drops `--code`/`-c` so a model that
/// keeps rewriting JS while leaving a bad `--scope` (NM-322 overnight
/// `sr-clear-transform[zh]` wedge) still accumulates strikes. Write-gate
/// PreferKnown fingerprints stay full-argv — only the failure breaker
/// collapses code churn.
pub(crate) fn failure_circuit_fingerprint(
    tool_name: &str,
    arguments: &serde_json::Value,
) -> String {
    let canonical =
        crate::agent::task_continuity::canonicalize_tool_arguments(tool_name, arguments);
    let real = crate::tools::resolve_tool_name(tool_name);
    if real.as_str() == "shell" {
        if let Some(cmd) = canonical.get("command").and_then(|v| v.as_str()) {
            if let Some(stripped) = strip_transform_code_flag_for_failure_circuit(cmd) {
                let mut adjusted = canonical.clone();
                if let Some(obj) = adjusted.as_object_mut() {
                    obj.insert("command".into(), serde_json::Value::String(stripped));
                }
                return canonical_tool_call_fingerprint_from_canonical(real.as_str(), &adjusted);
            }
        }
    }
    canonical_tool_call_fingerprint_from_canonical(real.as_str(), &canonical)
}

/// Drop `--code` / `-c` (and their values) from `neomind transform create|update`
/// argv so failure-circuit strikes ignore JS rewrites. Returns `None` when the
/// command is not that family (caller keeps the original fingerprint).
fn strip_transform_code_flag_for_failure_circuit(cmd: &str) -> Option<String> {
    let Ok(argv) = crate::toolkit::shell::tokenize_neomind_command(cmd.trim()) else {
        return None;
    };
    let (domain, action) = neomind_cli_ops::json_arg::domain_action_from_argv(&argv)?;
    if domain != "transform" || (action != "create" && action != "update") {
        return None;
    }
    let mut out = Vec::with_capacity(argv.len());
    let mut i = 0;
    while i < argv.len() {
        let a = argv[i].as_str();
        if a == "--code" || a == "-c" {
            i += 2; // skip flag + value (best-effort; missing value still advances past flag)
            continue;
        }
        if a.starts_with("--code=") || a.starts_with("-c=") {
            i += 1;
            continue;
        }
        out.push(argv[i].clone());
        i += 1;
    }
    // Stable token serialization (preserve multi-word arg boundaries).
    Some(serde_json::to_string(&out).unwrap_or_else(|_| out.join(" ")))
}

fn canonical_tool_call_fingerprint_from_canonical(
    real_tool_name: &str,
    canonical: &serde_json::Value,
) -> String {
    match real_tool_name {
        "shell" => {
            let cmd = canonical
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let body = shell_command_fingerprint_body(cmd, canonical);
            if !body.is_empty() {
                super::super::task_continuity::write_fingerprint("shell", body.as_str())
            } else if canonical
                .as_object()
                .is_some_and(|m| m.iter().any(|(k, v)| k != "command" && !v.is_null()))
            {
                let args = super::super::task_continuity::stable_json_string(canonical);
                super::super::task_continuity::write_fingerprint("shell", &args)
            } else {
                super::super::task_continuity::write_fingerprint("shell", "<empty>")
            }
        }
        other => {
            let mut for_fp = canonical.clone();
            if let Some(obj) = for_fp.as_object_mut() {
                obj.remove(crate::agent::task_continuity::PREPARED_OPERATION_ARG);
                obj.remove(crate::agent::task_continuity::HOST_PREPARED_OPERATION_ARG);
            }
            let args = super::super::task_continuity::stable_json_string(&for_fp);
            super::super::task_continuity::write_fingerprint(other, &args)
        }
    }
}

/// Fingerprint for a mutating tool call, if this call is a write.
///
/// **Single source of truth:** canonicalize once (lifecycle flag stripped
/// **before** CLI serialize), then
/// [`crate::agent::task_continuity::derive_tool_capability_resolved`] on the
/// resolved tool + canonical args — never re-enter domain action inference.
/// Read-only capability → `None` (never gated). **Any** non-read-only path
/// (proven CLI mutation, unknown shell, unknown extension tool) returns
/// `Some` so the durable write-intent gate cannot be skipped.
///
/// Shell fingerprints keep the original command string (arg values are not
/// lowercased) so `DeviceA` and `devicea` do not collapse into one gate key.
pub(crate) fn mutating_write_fingerprint(
    tool_name: &str,
    arguments: &serde_json::Value,
) -> Option<String> {
    // canonicalize_tool_arguments strips `_new_operation` first, then maps
    // CLI aliases — so the flag never enters the business fingerprint.
    let canonical =
        crate::agent::task_continuity::canonicalize_tool_arguments(tool_name, arguments);
    let real = crate::tools::resolve_tool_name(tool_name);
    let cap =
        crate::agent::task_continuity::derive_tool_capability_resolved(real.as_str(), &canonical);
    if cap.read_only {
        return None;
    }
    Some(canonical_tool_call_fingerprint_from_canonical(
        real.as_str(),
        &canonical,
    ))
}

/// Shell write-fingerprint body.
///
/// - Always strip host-meta tokens (`_new_operation`, prepared-operation-id)
///   so PreferKnown keys stay business-only regardless of flag placement.
/// - When a primary JSON flag / structured `params` is present, normalize to
///   `{argv, flag, params}` so object form and CLI string form share one key.
/// - Otherwise fingerprint stripped argv as stable JSON (token boundaries).
fn shell_command_fingerprint_body(cmd: &str, args: &serde_json::Value) -> String {
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let Ok(argv) = crate::toolkit::shell::tokenize_neomind_command(trimmed) else {
        return cmd.to_string();
    };
    let orig_len = argv.len();
    let stripped = crate::toolkit::shell::strip_host_meta_from_argv(argv);
    let meta_stripped = stripped.len() != orig_len;
    if let Some((base_argv, flag, params)) = primary_json_fingerprint_parts(&stripped, args) {
        return super::super::task_continuity::stable_json_string(&serde_json::json!({
            "argv": base_argv,
            "flag": flag,
            "params": params,
        }));
    }
    if !meta_stripped {
        // No host-meta tokens were stripped and no primary JSON payload is present:
        // keep the original command string so persisted ledger fingerprints remain
        // compatible across upgrades and quoted arg boundaries stay intact.
        return cmd.to_string();
    }
    // Host-meta tokens were stripped from a non-JSON command: when every remaining
    // token is plain (no whitespace or quotes), rejoin with spaces so it matches
    // the clean command string; otherwise serialize as an argv array to preserve
    // quoted token boundaries.
    if stripped.iter().all(|t| {
        !t.chars()
            .any(|c| c.is_whitespace() || c == '\'' || c == '"')
    }) {
        return stripped.join(" ");
    }
    serde_json::Value::Array(
        stripped
            .into_iter()
            .map(serde_json::Value::String)
            .collect(),
    )
    .to_string()
}

/// Split argv into business argv + primary JSON payload.
///
/// Structured shell `params` (object or array) and CLI `--params`/`--set`/
/// `--components`/`--ids`/… canonicalize to the same pair so write-gate
/// PreferKnown treats them as one operation.
fn primary_json_fingerprint_parts(
    argv: &[String],
    args: &serde_json::Value,
) -> Option<(Vec<String>, &'static str, serde_json::Value)> {
    let spec = neomind_cli_ops::json_arg::primary_json_spec(argv)?;
    let flag = spec.flag;
    let file_flag = format!("{flag}-file");
    let structured = args.get("params").and_then(|p| {
        if p.is_object() || p.is_array() {
            Some(p.clone())
        } else if p.is_null() {
            Some(match spec.kind {
                neomind_cli_ops::json_arg::ExpectedJsonKind::Object => serde_json::json!({}),
                neomind_cli_ops::json_arg::ExpectedJsonKind::Array => serde_json::json!([]),
            })
        } else {
            None
        }
    });

    let mut base = Vec::new();
    let mut cli_params: Option<serde_json::Value> = None;
    let mut i = 0;
    while i < argv.len() {
        let a = &argv[i];
        if a == flag || a.as_str() == file_flag {
            if let Some(raw) = argv.get(i + 1) {
                cli_params = if a.as_str() == file_flag {
                    std::fs::read_to_string(raw)
                        .ok()
                        .and_then(|t| neomind_cli_ops::json_arg::parse_json_arg_str(t.trim()).ok())
                } else {
                    neomind_cli_ops::json_arg::parse_json_arg_str(raw).ok()
                };
            }
            i += 2;
            continue;
        }
        base.push(a.clone());
        i += 1;
    }

    let params = match (structured, cli_params) {
        (Some(s), _) => s,
        (None, Some(c)) => c,
        (None, None) => return None,
    };
    Some((
        base,
        flag,
        if params.is_null() {
            match spec.kind {
                neomind_cli_ops::json_arg::ExpectedJsonKind::Object => serde_json::json!({}),
                neomind_cli_ops::json_arg::ExpectedJsonKind::Array => serde_json::json!([]),
            }
        } else {
            params
        },
    ))
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
    let Ok(argv) = crate::toolkit::shell::tokenize_neomind_command(cmd.trim()) else {
        // Fallback: whitespace split for non-quoted simple commands.
        let mut toks = cmd.split_whitespace();
        let first = toks.next()?;
        if !first.ends_with("neomind") {
            return None;
        }
        let rest: Vec<String> = std::iter::once("neomind".to_string())
            .chain(toks.map(str::to_string))
            .collect();
        return neomind_cli_ops::json_arg::domain_action_from_argv(&rest)
            .map(|(d, _)| d.to_lowercase());
    };
    neomind_cli_ops::json_arg::domain_action_from_argv(&argv).map(|(d, _)| d.to_lowercase())
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
    if classes.is_empty() || classes.contains(&hint_action) {
        return true;
    }
    // `neomind device control <id> <verb>` carries the verb in an ARGUMENT, so
    // it classifies as "control" while the user's ask classifies as the verb
    // ("use its stop command" -> disable/stop). Requiring an exact class match
    // made a SUCCESSFUL stop report "the requested action was NEVER executed"
    // (live 2026-10-21 eval: `device control pump-A stop`, exit 0, note fired).
    // A control mutation delivers the verb class when the verb is present as a
    // standalone token in the command.
    if classes.contains(&"control") {
        let has = |verb: &str| {
            seg_lower
                .split(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                .any(|t| t == verb)
        };
        match hint_action {
            "disable/stop" => return has("stop") || has("disable"),
            "enable/start" => return has("start") || has("enable"),
            _ => {}
        }
    }
    false
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

/// Strong guided-device markers (device noun already implied).
const DEVICE_ONBOARDING_STRONG_MARKERS: &[&str] = &[
    "连接设备",
    "连接传感器",
    "配置传感器",
    "connect my",
    "onboard",
];

/// Vague markers — only fire when a device/sensor noun co-occurs.
const DEVICE_ONBOARDING_VAGUE_MARKERS: &[&str] = &[
    "接入",
    "带我走",
    "connect a",
    "walk me through",
    "guide me through",
    "setting it up",
    "set it up",
];

const DEVICE_ONBOARDING_NOUNS: &[&str] = &[
    "device",
    "sensor",
    "设备",
    "传感器",
    "mqtt",
    "温度计",
    "摄像头",
];

/// Explicit non-device domains / external-broker phrasing — never hijack these.
const DEVICE_ONBOARDING_EXCLUSIONS: &[&str] = &[
    "connector",
    "external broker",
    "remote broker",
    "外部 broker",
    "外部 mqtt",
    "远程 mqtt",
    "远程broker",
    "webhook",
    "钉钉",
    "slack",
    "通知渠道",
    "channel",
    "deepseek",
    "大模型",
    "llm",
    "extension",
    "扩展",
    "rule",
    "规则",
    "dashboard",
    "仪表盘",
    "widget",
    "transform",
    "push",
];

/// True when the user wants guided device onboarding (MQTT/sensor connect),
/// not a one-off status/query or a cross-domain "connect/onboard" ask.
pub(crate) fn looks_like_guided_device_onboarding(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    if crate::agent::user_requested_no_execution(msg) {
        return false;
    }
    // Instructional "how do I connect…" stays explanation-only.
    if INSTRUCTIONAL_QUESTION_PATTERNS
        .iter()
        .any(|p| lower.contains(&p.to_lowercase()))
    {
        return false;
    }
    if DEVICE_ONBOARDING_EXCLUSIONS
        .iter()
        .any(|e| lower.contains(&e.to_lowercase()))
    {
        return false;
    }
    // Domain gate: when any domain keyword matched, device must be among them.
    let domains = matched_domains(msg);
    if !domains.is_empty() && !domains.contains(&"device") {
        return false;
    }
    let strong = DEVICE_ONBOARDING_STRONG_MARKERS
        .iter()
        .any(|m| lower.contains(&m.to_lowercase()));
    if strong {
        return true;
    }
    let vague = DEVICE_ONBOARDING_VAGUE_MARKERS
        .iter()
        .any(|m| lower.contains(&m.to_lowercase()));
    if !vague {
        return false;
    }
    DEVICE_ONBOARDING_NOUNS
        .iter()
        .any(|n| lower.contains(&n.to_lowercase()))
}

/// True when a device was already created this turn.
pub(crate) fn has_device_create(commands: &[&str]) -> bool {
    commands.iter().any(|c| {
        let l = c.to_lowercase();
        l.contains("device create")
    })
}

/// Mid-turn / turn-end redirect: guided MQTT onboarding without `device create`.
/// Same policy class as agent-conclusion redirect (default chat, independent
/// of `behavioral_guards`).
pub(crate) fn build_device_onboarding_redirect(
    user_message: &str,
    executed_commands: &[&str],
) -> Option<String> {
    if !looks_like_guided_device_onboarding(user_message) {
        return None;
    }
    if has_device_create(executed_commands) {
        return None;
    }
    // Another domain already mutated successfully — do not force device create.
    if executed_commands.iter().any(|c| is_mutation_command(c)) {
        let has_non_device_mutation = executed_commands.iter().any(|c| {
            is_mutation_command(c) && command_domain(c).map(|d| d != "device").unwrap_or(false)
        });
        if has_non_device_mutation {
            return None;
        }
    }

    let executed_summary = if executed_commands.is_empty() {
        "(no prior commands)".to_string()
    } else {
        executed_commands
            .iter()
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let has_types = executed_commands.iter().any(|c| {
        let l = c.to_lowercase();
        l.contains("device types")
    });

    let zh = crate::agent::text_prefers_chinese(user_message);
    let msg = if zh {
        let next = if has_types {
            "你已看过设备类型。现在必须用 shell 执行：\n\
             `neomind device create --name <名称> --device-type <类型> --adapter-type mqtt`\n\
             然后给出 MQTT broker/topic 连接信息。不要再反复 load skill / 列 connector。"
                .to_string()
        } else {
            "现在必须用 shell 依次执行：\n\
             1) `neomind device types list` — 发现可用类型\n\
             2) `neomind device create --name <名称> --device-type <类型> --adapter-type mqtt`\n\
             然后给出 MQTT broker/topic。不要走 connector create（嵌入式 broker 用 device create）。\
             不要再反复 load skill。"
                .to_string()
        };
        format!(
            "⚠️ 用户要引导式 MQTT 设备接入，但你还没有 `device create`。\n\n\
             已执行:\n{executed_summary}\n\n\
             {next}\n\
             不要输出最终回答。现在输出 tool call。"
        )
    } else {
        let next = if has_types {
            "You already listed device types. You MUST run via shell:\n\
             `neomind device create --name <name> --device-type <type> --adapter-type mqtt`\n\
             then give MQTT broker/topic connection info. Do NOT keep loading skills or listing connectors."
                .to_string()
        } else {
            "You MUST run via shell, in order:\n\
             1) `neomind device types list` — discover templates\n\
             2) `neomind device create --name <name> --device-type <type> --adapter-type mqtt`\n\
             then give MQTT broker/topic. Do NOT use connector create (embedded broker uses device create).\n\
             Do NOT keep loading skills."
                .to_string()
        };
        format!(
            "⚠️ The user asked for guided MQTT device onboarding, but you have NOT run `device create` yet.\n\n\
             Previously executed:\n{executed_summary}\n\n\
             {next}\n\
             Do NOT output a final answer. OUTPUT A TOOL CALL NOW."
        )
    };
    Some(msg)
}

/// Phrases that mark "is the watched thing OK right now?" status questions.
/// Deliberately tied to the agent-query-conclusion-first eval wording and
/// close cousins — not a generic "status" detector (that would hijack
/// device/health/extension checks).
const AGENT_STATUS_QUESTION_MARKERS: &[&str] = &[
    // Chinese (eval: 「冷库现在还好吗?」)
    "还好吗",
    "还好么",
    "还好嘛",
    "正常吗",
    "正常不",
    "怎么样了",
    "现在怎样",
    "现在如何",
    // English (eval: "Is the cold room OK right now?")
    "ok right now",
    "okay right now",
    "ok now",
    "okay now",
    "how's the",
    "how is the",
    "how're the",
    "still ok",
    "still okay",
];

/// Explicit non-agent domains — status questions about these must not redirect.
const AGENT_STATUS_EXCLUSIONS: &[&str] = &[
    "extension",
    "扩展",
    "connector",
    "settings",
    "设置",
    "rule",
    "规则",
    "dashboard",
    "仪表盘",
    "widget",
    "transform",
    "message",
    "通知",
    "system",
    "系统服务",
    "系统信息",
];

/// Explicit recheck asks — status redirect must NOT block a fresh invoke.
const AGENT_STATUS_RECHECK_MARKERS: &[&str] = &[
    "重新检查",
    "再查一次",
    "再跑一次",
    "重新跑",
    "recheck",
    "check again",
    "run again",
    "invoke",
    "fresh look",
    "fresh run",
];

fn ascii_word_boundary_at(s: &str, idx: usize) -> bool {
    if idx == 0 {
        return true;
    }
    let Some(b) = s.as_bytes().get(idx.saturating_sub(1)) else {
        return true;
    };
    !b.is_ascii_alphanumeric() && *b != b'_'
}

fn ascii_word_ends_at(s: &str, end: usize) -> bool {
    match s.as_bytes().get(end) {
        None => true,
        Some(b) => !b.is_ascii_alphanumeric() && *b != b'_',
    }
}

/// True when the user is asking whether something an agent already watches
/// is OK / what its current state is — not requesting a new mutation.
pub(crate) fn looks_like_agent_status_question(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    if AGENT_STATUS_RECHECK_MARKERS
        .iter()
        .any(|m| lower.contains(&m.to_lowercase()))
    {
        return false;
    }
    if AGENT_STATUS_EXCLUSIONS
        .iter()
        .any(|e| lower.contains(&e.to_lowercase()))
    {
        return false;
    }
    // When a concrete non-agent domain keyword matched, this is not an
    // agent-conclusion ask (e.g. "传感器 … 正常吗" → device get).
    // Bare "冷库还好吗" matches no DOMAIN_KEYWORDS and still qualifies.
    let domains = matched_domains(msg);
    if !domains.is_empty() && !domains.contains(&"agent") {
        return false;
    }
    if AGENT_STATUS_QUESTION_MARKERS
        .iter()
        .any(|m| lower.contains(&m.to_lowercase()))
    {
        return true;
    }
    // "Is the cold room OK?" / "Is fridge-1 okay?" — "is … ok/okay" with a
    // short gap and full ASCII word boundaries (avoid "This "/"his "/"redis ").
    let mut search = lower.as_str();
    let mut base = 0usize;
    while let Some(rel) = search.find("is ") {
        let is_at = base + rel;
        if ascii_word_boundary_at(&lower, is_at) {
            let after = &lower[is_at + 3..];
            for (ok_word, ok_len) in [("ok", 2usize), ("okay", 4usize)] {
                let mut asearch = after;
                let mut a_base = 0usize;
                while let Some(a_rel) = asearch.find(ok_word) {
                    let ok_at = a_base + a_rel;
                    // Require a space (or start) before ok, and a boundary after.
                    let preceded_ok = ok_at == 0
                        || after
                            .as_bytes()
                            .get(ok_at.saturating_sub(1))
                            .is_some_and(|b| b.is_ascii_whitespace());
                    if preceded_ok
                        && ascii_word_ends_at(after, ok_at + ok_len)
                        && ok_at <= 48
                        && !after[..ok_at].contains('?')
                    {
                        return true;
                    }
                    let next = ok_at + ok_len;
                    a_base = next;
                    asearch = &after[next..];
                }
            }
        }
        let next = is_at + 3;
        base = next;
        search = &lower[next..];
    }
    false
}

/// True when any shell command already read an agent conclusion / execution.
pub(crate) fn has_agent_conclusion_read(commands: &[&str]) -> bool {
    commands.iter().any(|c| {
        let l = c.to_lowercase();
        l.contains("agent latest-execution")
            || l.contains("agent executions")
            // Legacy rehydrated chat tool name (removed from model surface;
            // still counts if an old session replays it).
            || l.contains("query_conclusion")
    })
}

/// True when a shell command already ran `neomind agent list`.
pub(crate) fn has_agent_list_command(commands: &[&str]) -> bool {
    commands.iter().any(|c| {
        let l = c.to_lowercase();
        l.contains("agent list")
    })
}

/// True when a tool result looks like an empty `agent list` payload.
pub(crate) fn agent_list_result_looks_empty(result: &str) -> bool {
    let compact: String = result.chars().filter(|c| !c.is_whitespace()).collect();
    let lower = compact.to_lowercase();
    lower.contains("\"agents\":[]")
        || lower.contains("\"total\":0")
        || lower.contains("'agents':[]")
}

/// True when `agent list` already returned an empty roster (0 agents).
pub(crate) fn saw_empty_agent_list(
    executed_commands: &[&str],
    tool_results: &[(String, String)],
) -> bool {
    if !has_agent_list_command(executed_commands) {
        return false;
    }
    tool_results
        .iter()
        .any(|(_name, res)| agent_list_result_looks_empty(res))
}

/// Test helper: redirect without tool-result context (empty results).
#[cfg(test)]
pub(crate) fn build_agent_conclusion_redirect(
    user_message: &str,
    executed_commands: &[&str],
) -> Option<String> {
    build_agent_conclusion_redirect_with_results(user_message, executed_commands, &[])
}

/// Mid-turn / turn-end redirect: status question about a watched thing, but
/// the model never read `agent latest-execution` (typical detour: empty
/// `device list` / `system info` → fabricated "all good").
///
/// Independent of `behavioral_guards` — same policy class as NM-250
/// foreshadow continue (default chat must recover without flipping the
/// full coercion suite). Returns `None` when the ask is not a status
/// question, a conclusion was already read, or `agent list` already
/// returned an empty roster (nothing to `latest-execution`).
pub(crate) fn build_agent_conclusion_redirect_with_results(
    user_message: &str,
    executed_commands: &[&str],
    tool_results: &[(String, String)],
) -> Option<String> {
    if !looks_like_agent_status_question(user_message) {
        return None;
    }
    if crate::agent::user_requested_no_execution(user_message) {
        return None;
    }
    if has_agent_conclusion_read(executed_commands) {
        return None;
    }
    if saw_empty_agent_list(executed_commands, tool_results) {
        return None;
    }

    let executed_summary = if executed_commands.is_empty() {
        "(no prior commands)".to_string()
    } else {
        executed_commands
            .iter()
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let zh = crate::agent::text_prefers_chinese(user_message);
    let msg = if zh {
        format!(
            "⚠️ 用户在问「某个已有 agent 盯着的东西现在还好吗」。\
             你还没有读取任何 agent 结论。不要用空的 `device list` / `system info` / health 编造状态。\n\n\
             已执行:\n{executed_summary}\n\n\
             现在必须用 shell 依次执行：\n\
             1) `neomind agent list` — 按名称匹配相关 agent\n\
             2) `neomind agent latest-execution <ID>` — 读取最近一次结论（若尚无执行记录，如实说明）\n\
             不要 `agent invoke`，除非用户明确要求重新检查。\n\
             不要输出最终回答。现在输出 tool call。"
        )
    } else {
        format!(
            "⚠️ The user asked whether something an agent already watches is OK right now. \
             You have NOT read any agent conclusion yet. Do NOT invent a reading from an empty \
             `device list` / `system info` / health.\n\n\
             Previously executed:\n{executed_summary}\n\n\
             You MUST run via shell, in order:\n\
             1) `neomind agent list` — match the agent by name\n\
             2) `neomind agent latest-execution <ID>` — read the latest conclusion \
             (or state clearly it has not run yet)\n\
             Do NOT `agent invoke` unless the user asked for a fresh recheck.\n\
             Do NOT output a final answer. OUTPUT A TOOL CALL NOW."
        )
    };
    Some(msg)
}

/// The mid-turn directive for a repeatedly promised, never executed
/// command. Advisory in tone and actionable: re-run the exact command the
/// model itself stated, or name what is missing — never a bare "do it".
///
/// ONE language per call (`language_hint` = the narration that stated the
/// promise): a directive that states everything twice, once per language,
/// makes small models drift into the language they read last.
pub(crate) fn build_unkept_promise_directive(
    cmd: &str,
    rounds: usize,
    language_hint: &str,
) -> String {
    if crate::agent::text_prefers_chinese(language_hint) {
        format!(
            "⚠️ 你最近 {rounds} 轮回复都说明要执行 `{cmd}`，但它从未被执行，反而运行了其他工具。\
             现在就用 shell 工具执行它（直接复用你陈述过的完整命令，包含其中的标识符），\
             或用一句话说明缺少哪个标识/信息。不要再次复述计划，也不要先运行其他工具。"
        )
    } else {
        format!(
            "⚠️ STATED-INTENT MISMATCH: your last {rounds} replies each said you would run \
             `{cmd}` — but it was never executed, while other tools ran instead. Either run \
             `{cmd}` NOW with the `shell` tool (re-use the exact full command you already \
             stated, including its identifiers), or reply with ONE sentence naming the \
             identifier/information that is missing. Do not restate the plan and do not \
             run other tools first."
        )
    }
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

    /// Instructional questions carry action verbs but request an
    /// EXPLANATION, not execution: "如何创建设备？" answered with a correct
    /// how-to was being labelled "the requested action was NOT executed …
    /// the reply above is a progress report" (same over-broad-pattern family
    /// as the 2026-10-20 narration incident).
    #[test]
    fn instructional_questions_do_not_request_action() {
        assert!(!user_message_requires_action("如何创建设备？"));
        assert!(!user_message_requires_action("怎么删除一条规则"));
        assert!(!user_message_requires_action("怎样启用监控最稳妥"));
        assert!(!user_message_requires_action("为什么发送消息失败了？"));
        assert!(!user_message_requires_action("How do I create a device?"));
        assert!(!user_message_requires_action(
            "how to add a widget to the dashboard"
        ));
        // The turn-end completeness note stays silent for them.
        assert!(build_incomplete_action_note("如何创建设备？", &[], &[]).is_none());
        assert!(build_incomplete_action_note("How do I create a device?", &[], &[]).is_none());
        // Genuine requests phrased as questions still trigger — the gate
        // must not swallow "can you …?" asks.
        assert!(user_message_requires_action("你能帮我创建一个设备吗？"));
        assert!(user_message_requires_action(
            "Can you create a device for me?"
        ));
        assert!(build_incomplete_action_note("你能帮我创建一个设备吗？", &[], &[]).is_some());
        // Position matters: a marker AFTER the verb is filler inside a real
        // request, not a question about it.
        assert!(user_message_requires_action(
            "帮我删除这个设备，不知道怎么操作"
        ));
        assert!(user_message_requires_action(
            "delete the stale rule, I don't remember how to do it"
        ));
    }

    /// "先不要执行，只告诉我步骤" contains an action verb but asks for a
    /// PLAN: both the completeness note and the dead-end coercion must stay
    /// silent — coercing execution would contradict the user.
    #[test]
    fn plan_only_requests_skip_action_guards() {
        let msg = "帮我创建设备，先不要执行，只告诉我步骤";
        assert!(user_message_requires_action(msg));
        assert!(build_incomplete_action_note(msg, &[], &[]).is_none());
        assert!(build_list_only_dead_end_prompt(msg, &["neomind device list"], &[], 3,).is_none());
        // The same request WITHOUT the plan-only clause still flags.
        assert!(build_incomplete_action_note("帮我创建设备", &[], &[]).is_some());
    }

    /// L3 Spark: read-only asks that forbid mutation ("不要修改配置") must not
    /// trip `incomplete_action_note` just because "修改" is an ACTION_VERB.
    #[test]
    fn negated_mutation_verbs_do_not_request_action() {
        assert!(!user_message_requires_action(
            "请再分别查看 weather-forecast-v2 扩展的当前配置和最近日志。只读查询，不要修改配置。"
        ));
        assert!(!user_message_requires_action(
            "请查询仪表盘详情，告诉我它当前有几个组件，不要做任何修改。"
        ));
        assert!(!user_message_requires_action(
            "列出所有仪表盘并加载技能概览，简短总结即可，不要修改任何内容。"
        ));
        assert!(!user_message_requires_action(
            "Inspect the extension config only — don't update anything."
        ));
        assert!(build_incomplete_action_note(
            "只读查询扩展配置，不要修改配置。",
            &["neomind extension config weather-forecast-v2"],
            &[],
        )
        .is_none());

        // Mixed: an un-negated create still requests action even if a later
        // clause forbids a different mutation.
        assert!(user_message_requires_action(
            "帮我创建一个 webhook 通道，不要修改已有配置。"
        ));
        assert!(build_incomplete_action_note(
            "帮我创建一个 webhook 通道，不要修改已有配置。",
            &[],
            &[],
        )
        .is_some());
    }

    /// Stage 1 adversarial suite: English word boundaries, Chinese
    /// completed-aspect / noun-suffix exclusions, long-range negation,
    /// interrogative/hypothetical demotion.
    #[test]
    fn stage1_action_verb_adversarial_exclusions() {
        // English substring false positives
        assert!(!user_message_requires_action("Show device IP address"));
        assert!(!user_message_requires_action("check padding on the widget"));
        assert!(!user_message_requires_action("additional sensors online?"));
        assert!(!user_message_requires_action(
            "sender name for notifications"
        ));
        assert!(!user_message_requires_action("startup sequence looks fine"));
        assert!(!user_message_requires_action("service restart history"));
        assert!(!user_message_requires_action("stopwatch reading now"));
        assert!(!user_message_requires_action("list created rules"));
        assert!(!user_message_requires_action(
            "show the creator of this agent"
        ));
        assert!(!user_message_requires_action("updated_at field value"));
        assert!(!user_message_requires_action("controller firmware version"));
        assert!(!user_message_requires_action(
            "shared dashboard link please"
        ));
        assert!(!user_message_requires_action("list installed extensions"));
        assert!(!user_message_requires_action("show created rules"));
        assert!(!user_message_requires_action("check enabled agents"));
        assert!(!user_message_requires_action("is the extension installed?"));
        assert!(!user_message_requires_action("did it create a rule?"));
        assert!(!user_message_requires_action(
            "what happens if I delete this device?"
        ));
        assert!(!user_message_requires_action("what if I remove the rule?"));

        // Chinese completed-aspect / status
        assert!(!user_message_requires_action("查看已安装的扩展"));
        assert!(!user_message_requires_action("列出已创建的规则"));
        assert!(!user_message_requires_action("哪些设备已启用"));
        assert!(!user_message_requires_action("显示已绑定的数据源"));
        assert!(!user_message_requires_action("扩展有没有安装？"));
        assert!(!user_message_requires_action("设备是否已经绑定规则？"));
        assert!(!user_message_requires_action("规则是否启用了？"));

        // Chinese noun-suffix compounds
        assert!(!user_message_requires_action("查看扩展的安装路径"));
        assert!(!user_message_requires_action("看看设备的更新日志"));
        assert!(!user_message_requires_action("仪表盘的分享链接在哪"));
        assert!(!user_message_requires_action("规则的创建时间是什么"));
        assert!(!user_message_requires_action("设备的控制面板怎么进"));
        assert!(!user_message_requires_action("告诉我删除记录"));
        assert!(!user_message_requires_action("扩展安装说明在哪"));

        // Long-distance / expanded negation
        assert!(!user_message_requires_action("不要对任何仪表盘做修改"));
        assert!(!user_message_requires_action(
            "只读查询配置，未修改任何内容"
        ));
        assert!(!user_message_requires_action(
            "read-only inspection, no changes please"
        ));
        assert!(!user_message_requires_action(
            "never update the production rule"
        ));
        assert!(!user_message_requires_action(
            "please don't touch any connectors"
        ));

        // Still true write requests
        assert!(user_message_requires_action("create a new device"));
        assert!(user_message_requires_action(
            "add a widget to the dashboard"
        ));
        assert!(user_message_requires_action("start the connector now"));
        assert!(user_message_requires_action("安装 weather 扩展"));
        assert!(user_message_requires_action("删除这条规则"));
        assert!(user_message_requires_action("启用监控代理"));
        assert!(user_message_requires_action("更新仪表盘标题"));
        assert!(user_message_requires_action("帮我绑定这个数据源"));

        // P1-6: repeated CJK verb after an excluded first mention, and clause punctuation
        // stopping completed-aspect lookback.
        assert!(user_message_requires_action(
            "查看已安装的扩展并安装weather"
        ));
        assert!(user_message_requires_action("已连接，请删除"));
        assert!(user_message_requires_action("已启用。请更新配置"));
        assert!(user_message_requires_action("已绑定; 删除规则"));
        assert!(user_message_requires_action("已安装, 请启用它"));
    }

    /// Live 2026-10-21: "Send it a set_alarm command" extracts action=write/send
    /// with NO domain, while `device control fridge-3 set_alarm` succeeded.
    /// A domain-less ask must not be labelled "not executed".
    #[test]
    fn domain_less_ask_never_gets_a_completeness_note() {
        let msg = "fridge-3's temperature is 8.2\u{00b0}C - it violates the cold-chain limit, \
                   food-safety risk. Send it a set_alarm command to turn the alarm on.";
        let cmd = "neomind device control fridge-3 set_alarm 2>&1";
        assert_eq!(
            extract_action_parts(msg).0,
            "",
            "fixture must have no domain"
        );
        assert!(build_incomplete_action_note(msg, &[cmd], &[cmd.to_string()]).is_none());
        assert!(build_incomplete_action_note(msg, &[], &[]).is_none());
    }

    /// Live 2026-10-21: "use its stop command to shut it down" + a successful
    /// `neomind device control pump-A stop` (exit 0) still produced "the
    /// requested action was NEVER executed" — the ask classifies as
    /// disable/stop, the command as control, and an exact-class match failed.
    #[test]
    fn control_verb_delivers_stop_and_start_actions() {
        let msg = "pump-A's vibration is too high \u{2014} use its stop command to shut it down.";
        assert_eq!(extract_action_parts(msg).1, "disable/stop");
        let stop = "neomind device control pump-A stop 2>&1";
        assert!(
            mutation_satisfies_action(stop, "", "disable/stop"),
            "{stop}"
        );
        assert!(build_incomplete_action_note(
            msg,
            &["neomind device list 2>&1", stop],
            &[stop.to_string()],
        )
        .is_none());
        // Symmetric: a start request delivered by `control ... start`.
        let start = "neomind device control pump-A start";
        assert!(mutation_satisfies_action(start, "", "enable/start"));
        // A control mutation whose verb does NOT match the ask still fails.
        let other = "neomind device control valve-1 set_valve open";
        assert!(!mutation_satisfies_action(other, "", "disable/stop"));
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
        assert!(build_list_only_dead_end_prompt(msg, &cmds, &[], 1).is_none());
    }

    #[test]
    fn test_named_action_still_injects() {
        // Guard must not over-suppress: a genuine action request with a
        // nameable action still injects — on the SECOND consecutive
        // read-only round (streak=1 = one prior read-only round).
        let msg = "创建一个温湿度仪表盘";
        let cmds = ["neomind device list", "neomind widget list"];
        assert!(build_list_only_dead_end_prompt(msg, &cmds, &[], 1).is_some());
        // First read-only round → repetition gate holds the injection.
        assert!(build_list_only_dead_end_prompt(msg, &cmds, &[], 0).is_none());
    }

    /// Coercion evidence gate (live 2026-10-19): the dead-end injection is a
    /// MUST-directive into the model's context, so it must not fire on one
    /// exploratory round — `device list` + `dashboard list` right after the
    /// ask was the CORRECT first step of the flow that built the dashboard,
    /// and the round-1 injection demanding a phantom action hijacked the
    /// turn for 20 minutes. On a genuine stall (second consecutive
    /// read-only round) the guard still fires — now demanding the RIGHT
    /// action, thanks to the DOMAIN_KEYWORDS priority table.
    #[test]
    fn dead_end_needs_repetition_not_first_round_exploration() {
        // The incident's round 1: ID discovery, not a dead end → no
        // injection (old behavior: demanded "neomind device create").
        assert!(build_list_only_dead_end_prompt(
            "创建一个展示设备图片的仪表盘",
            &["neomind device list 2>&1", "neomind dashboard list 2>&1"],
            &[],
            0,
        )
        .is_none());

        // Same message, genuinely stalled (streak ≥1) → injects, and the
        // demanded action is the CORRECT one — the incident's phantom
        // "device create" is gone.
        let msg = build_list_only_dead_end_prompt(
            "创建一个展示设备图片的仪表盘",
            &["neomind device list", "neomind device get demo-001"],
            &[],
            1,
        )
        .expect("genuine stall must still inject");
        assert!(
            msg.contains("neomind dashboard create"),
            "must demand the correct domain, got: {msg}"
        );
        assert!(!msg.contains("device create"), "phantom demand gone: {msg}");

        // Unambiguous English ask behaves identically.
        assert!(build_list_only_dead_end_prompt(
            "add it to my dashboard",
            &["neomind dashboard list"],
            &[],
            0,
        )
        .is_none());
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

    /// Live incident (2026-10-19): a dashboard-of-device-images request
    /// named BOTH 设备 and 仪表盘. The old first-keyword chain extracted
    /// `device`, so the dead-end guard chased a phantom `neomind device
    /// create` all turn, and after `dashboard create` + `add-components`
    /// SUCCEEDED the turn still ended with a false "requested ACTION never
    /// executed" completeness note. The specific artifact domain now wins.
    #[test]
    fn multi_domain_request_prefers_artifact_over_ambient_device() {
        assert_eq!(
            extract_action_parts("创建一个展示设备图片的仪表盘"),
            ("dashboard", "create")
        );
        assert_eq!(
            extract_action_parts("create a dashboard showing device images"),
            ("dashboard", "create")
        );
        // The dead-end hint follows the corrected domain.
        assert_eq!(
            extract_action_hint("创建一个展示设备图片的仪表盘"),
            "neomind dashboard create"
        );
        // The delivered dashboard mutations now satisfy the request — no
        // false completeness note after the task ran.
        assert!(build_incomplete_action_note(
            "创建一个展示设备图片的仪表盘",
            &["neomind dashboard add-components d1 --components '[...]'"],
            &[
                "neomind dashboard create --name x".to_string(),
                "neomind dashboard add-components d1 --components '[...]'".to_string(),
            ],
        )
        .is_none());
        // Single-domain requests keep the old extraction.
        assert_eq!(extract_action_parts("创建一个新设备"), ("device", "create"));
        assert_eq!(
            extract_action_parts("为设备创建告警规则"),
            ("rule", "create")
        );
        // A dashboard request answered only by device mutations still
        // flags — the fix disambiguates the ASK, it does not excuse misses.
        assert!(build_incomplete_action_note(
            "创建一个展示设备图片的仪表盘",
            &["neomind device create foo"],
            &["neomind device create foo".to_string()],
        )
        .is_some());
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
            "neomind widget create \"番茄时钟\" --widget-type pomodoro-clock",
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
                "neomind widget create \"番茄时钟\" --widget-type pomodoro-clock",
                "neomind widget install gauge-3832",
            ],
            &["neomind widget create \"番茄时钟\" --widget-type pomodoro-clock".to_string()],
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
            1,
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
            1,
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

    #[test]
    fn failure_circuit_collapses_transform_code_churn_keeps_scope() {
        // Overnight wedge: model rewrote --code while leaving --scope device.
        let a = serde_json::json!({
            "command": "neomind transform create --name batt --scope device --code \"return input.battery * 100\""
        });
        let b = serde_json::json!({
            "command": "neomind transform create --name batt --scope device --code \"function f(input){ return input.battery * 100; }\""
        });
        let c = serde_json::json!({
            "command": "neomind transform create --name batt --scope device:sensor-001 --code \"return input.battery * 100\""
        });
        assert_eq!(
            failure_circuit_fingerprint("shell", &a),
            failure_circuit_fingerprint("shell", &b),
            "code-only churn must share one failure-circuit key"
        );
        assert_ne!(
            failure_circuit_fingerprint("shell", &a),
            failure_circuit_fingerprint("shell", &c),
            "scope change must be a different failure-circuit key"
        );
        // PreferKnown / canonical fingerprint still distinguishes code.
        assert_ne!(
            canonical_tool_call_fingerprint("shell", &a),
            canonical_tool_call_fingerprint("shell", &b),
        );
    }

    #[test]
    fn strip_transform_code_flag_only_for_create_update() {
        assert!(strip_transform_code_flag_for_failure_circuit("neomind transform list").is_none());
        let stripped = strip_transform_code_flag_for_failure_circuit(
            "neomind transform create --name x --scope global --code \"return 1\"",
        )
        .expect("create");
        assert!(!stripped.contains("--code"), "{stripped}");
        // Fingerprint may be JSON-serialized argv (preserves multi-word args).
        assert!(
            stripped.contains("--scope") && stripped.contains("global"),
            "{stripped}"
        );
        let stripped_eq = strip_transform_code_flag_for_failure_circuit(
            "neomind transform create --name x --scope global -c=\"return 1\"",
        )
        .expect("-c= form");
        assert!(!stripped_eq.contains("-c="), "{stripped_eq}");
        assert!(!stripped_eq.contains("return 1"), "{stripped_eq}");
    }

    /// The mutation-pattern audit (against the full live CLI surface):
    /// every state-changing subcommand must register as a mutation, and
    /// read-only commands must not — the bare " push" shell pattern used
    /// to match `neomind push list`, killing dead-end detection for the
    /// whole push domain, while market-install / push start / llm
    /// activate / draft approve successes went entirely untracked.
    #[test]
    fn mutating_write_fingerprint_covers_shell_and_direct_writes() {
        let shell_mut = serde_json::json!({
            "command": "neomind device control sensor-001 reboot"
        });
        let fp = mutating_write_fingerprint("shell", &shell_mut).expect("shell mutation");
        assert!(fp.starts_with("shell|"));
        assert!(fp.contains("control"));

        let shell_ro = serde_json::json!({"command": "neomind device list"});
        assert!(mutating_write_fingerprint("shell", &shell_ro).is_none());

        let fw = serde_json::json!({"path": "/tmp/a", "content": "x"});
        let fp2 = mutating_write_fingerprint("file_write", &fw).expect("file_write");
        assert!(fp2.starts_with("file_write|"));

        let dc = serde_json::json!({
            "device": "cam-1",
            "command": "render_text",
            "params": {"text": "hello"}
        });
        let fp_dc = mutating_write_fingerprint("device_command", &dc).expect("device_command");
        assert!(fp_dc.starts_with("device_command|"));
        assert!(fp_dc.contains("cam-1"));

        // Missing/unknown skill action fails closed into the write gate; RO
        // search/load must stay fingerprint-free.
        assert!(mutating_write_fingerprint(
            "skill",
            &serde_json::json!({"action": "load", "name": "x"})
        )
        .is_none());
    }

    /// Quoted `--param` value that embeds another `--param …` must not collide
    /// with the two-flag form after any PreparedInvoke token stripping.
    #[test]
    fn shell_write_fingerprint_preserves_quoted_param_boundaries() {
        let quoted = serde_json::json!({
            "command": "neomind device control D render_text --param 'text=a --param color=black'"
        });
        let split = serde_json::json!({
            "command": "neomind device control D render_text --param text=a --param color=black"
        });
        let fp_quoted = mutating_write_fingerprint("shell", &quoted).expect("quoted write");
        let fp_split = mutating_write_fingerprint("shell", &split).expect("split write");
        assert_ne!(
            fp_quoted, fp_split,
            "quoted single --param value must not share a write fingerprint with two --param flags"
        );
    }

    /// Chat `--prepared-operation-id` must not enter the business fingerprint:
    /// same extension invoke with different tokens → identical write key.
    #[test]
    fn shell_extension_invoke_fingerprint_ignores_prepared_operation_id() {
        let a = serde_json::json!({
            "command": "neomind extension invoke weather-forecast-v2 get_weather --params '{\"city\":\"Xiamen\"}' --prepared-operation-id aaa-111"
        });
        let b = serde_json::json!({
            "command": "neomind extension invoke weather-forecast-v2 get_weather --params '{\"city\":\"Xiamen\"}' --prepared-operation-id bbb-222"
        });
        let fp_a = mutating_write_fingerprint("shell", &a).expect("ext invoke a");
        let fp_b = mutating_write_fingerprint("shell", &b).expect("ext invoke b");
        assert_eq!(
            fp_a, fp_b,
            "PreparedInvoke id must not splinter the business write fingerprint"
        );
        // Sanity: a different business arg still diverges.
        let c = serde_json::json!({
            "command": "neomind extension invoke weather-forecast-v2 get_weather --params '{\"city\":\"Beijing\"}' --prepared-operation-id aaa-111"
        });
        let fp_c = mutating_write_fingerprint("shell", &c).expect("ext invoke c");
        assert_ne!(
            fp_a, fp_c,
            "different params must keep distinct fingerprints"
        );
    }

    /// NM-247: shell `params` object and `--params` JSON share one write key.
    #[test]
    fn shell_structured_params_fingerprint_equals_cli_params() {
        // Quote the JSON so tokenize keeps one --params value (same as live CLI).
        let via_flag = serde_json::json!({
            "command": "neomind extension invoke weather-forecast-v2 get_weather --params '{\"city\":\"Xiamen\"}'"
        });
        let via_object = serde_json::json!({
            "command": "neomind extension invoke weather-forecast-v2 get_weather",
            "params": {"city": "Xiamen"}
        });
        let fp_a = mutating_write_fingerprint("shell", &via_flag).expect("flag form");
        let fp_b = mutating_write_fingerprint("shell", &via_object).expect("object form");
        assert_eq!(
            fp_a, fp_b,
            "structured params must canonicalize to the same write fingerprint as --params"
        );
        // CJK / nested / bool shapes stay equal across forms.
        let nested_flag = serde_json::json!({
            "command": "neomind extension invoke ext-x cmd --params '{\"q\":\"厦门\",\"n\":2,\"ok\":true,\"tags\":[\"a\"],\"meta\":{\"k\":1}}'"
        });
        let nested_obj = serde_json::json!({
            "command": "neomind extension invoke ext-x cmd",
            "params": {"q":"厦门","n":2,"ok":true,"tags":["a"],"meta":{"k":1}}
        });
        assert_eq!(
            mutating_write_fingerprint("shell", &nested_flag).unwrap(),
            mutating_write_fingerprint("shell", &nested_obj).unwrap()
        );
    }

    /// Host meta leaked into `command` must not splinter PreferKnown keys, and
    /// dashboard `--set` / structured `params` must share one write fingerprint.
    #[test]
    fn shell_host_meta_and_dashboard_set_fingerprint_hygiene() {
        let clean = serde_json::json!({
            "command": "neomind dashboard update-component dash-1 --component-id c1 --set '{\"position\":{\"w\":2,\"h\":2}}'"
        });
        let polluted = serde_json::json!({
            "command": "neomind dashboard update-component dash-1 --component-id c1 --set '{\"position\":{\"w\":2,\"h\":2}}' _new_operation=true"
        });
        let flagged = serde_json::json!({
            "command": "neomind dashboard update-component dash-1 --component-id c1 --set '{\"position\":{\"w\":2,\"h\":2}}'",
            "_new_operation": true
        });
        let via_object = serde_json::json!({
            "command": "neomind dashboard update-component dash-1 --component-id c1",
            "params": {"position":{"w":2,"h":2}}
        });
        let fp_clean = mutating_write_fingerprint("shell", &clean).expect("clean");
        let fp_polluted = mutating_write_fingerprint("shell", &polluted).expect("polluted");
        let fp_flagged = mutating_write_fingerprint("shell", &flagged).expect("flagged");
        let fp_obj = mutating_write_fingerprint("shell", &via_object).expect("object");
        assert_eq!(
            fp_clean, fp_polluted,
            "_new_operation inside command must not change business fingerprint"
        );
        assert_eq!(
            fp_clean, fp_flagged,
            "_new_operation tool field must not change business fingerprint"
        );
        assert_eq!(
            fp_clean, fp_obj,
            "dashboard --set and structured params must share one fingerprint"
        );
    }

    #[test]
    fn shell_array_params_and_cli_flag_share_stable_fingerprint() {
        let add_cli = serde_json::json!({
            "command": "neomind dashboard add-components dash-1 --components '[{\"type\":\"value-card\",\"id\":\"c1\"}]'"
        });
        let add_arr = serde_json::json!({
            "command": "neomind dashboard add-components dash-1",
            "params": [{"id": "c1", "type": "value-card"}]
        });
        assert_eq!(
            mutating_write_fingerprint("shell", &add_cli).unwrap(),
            mutating_write_fingerprint("shell", &add_arr).unwrap(),
            "add-components CLI string and structured array params must share one key regardless of object key order"
        );

        let rem_cli = serde_json::json!({
            "command": "neomind dashboard remove-components dash-1 --ids '[\"c1\",\"c2\"]'"
        });
        let rem_arr = serde_json::json!({
            "command": "neomind dashboard remove-components dash-1",
            "params": ["c1", "c2"]
        });
        assert_eq!(
            mutating_write_fingerprint("shell", &rem_cli).unwrap(),
            mutating_write_fingerprint("shell", &rem_arr).unwrap(),
            "remove-components CLI string and structured array params must share one key"
        );

        // Read-only commands with leaked _new_operation must return None (never enter write gate).
        let ro_polluted = serde_json::json!({
            "command": "neomind extension logs weather-forecast-v2 _new_operation=true"
        });
        assert_eq!(
            mutating_write_fingerprint("shell", &ro_polluted),
            None,
            "RO command with leaked _new_operation must not enter write gate"
        );
    }

    #[test]
    fn device_command_is_mutating_write_and_gates() {
        use crate::agent::task_continuity::{
            derive_tool_capability, inject_new_operation_protocol, NEW_OPERATION_ARG,
        };

        let args = serde_json::json!({
            "device": "speaker-9",
            "command": "play_cue",
            "params": {"cue": "chime"}
        });
        let cap = derive_tool_capability("device_command", &args);
        assert!(!cap.read_only, "device_command must not be read-only");
        assert!(
            !cap.idempotent,
            "device_command must be non-idempotent mutating write"
        );
        assert!(
            matches!(
                cap.retry_policy,
                crate::agent::task_continuity::RetryPolicy::OnFailure { max_attempts: 2 }
            ),
            "device_command must use mutating_write retry policy, got {:?}",
            cap.retry_policy
        );
        let fp = mutating_write_fingerprint("device_command", &args).expect("fp");
        let mut with_flag = args.clone();
        with_flag
            .as_object_mut()
            .unwrap()
            .insert(NEW_OPERATION_ARG.into(), serde_json::json!(true));
        let fp2 = mutating_write_fingerprint("device_command", &with_flag).expect("fp+flag");
        assert_eq!(fp, fp2, "_new_operation must not change business fp");

        let mut defs = vec![crate::toolkit::tool::ToolDefinition {
            name: "device_command".into(),
            description: "Send a native command".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "device": {"type": "string"},
                    "command": {"type": "string"},
                    "params": {"type": "object"}
                },
                "required": ["device", "command"]
            }),
            example: None,
            category: crate::toolkit::tool::ToolCategory::System,
            scenarios: vec![],
            relationships: Default::default(),
            deprecated: false,
            replaced_by: None,
            version: "1.0.0".into(),
            examples: vec![],
            response_format: None,
            namespace: None,
        }];
        inject_new_operation_protocol(&mut defs);
        assert!(
            defs[0]
                .parameters
                .get("properties")
                .and_then(|p| p.get(NEW_OPERATION_ARG))
                .is_some(),
            "device_command schema must advertise _new_operation"
        );
    }

    /// GPT tip 5d8ed5f0: non-CLI writes + compound / unknown shell must not
    /// skip the execution-intent gate (no keyword-absence ⇒ read-only).
    #[test]
    fn unknown_and_compound_shell_cannot_skip_write_gate() {
        use crate::agent::task_continuity::derive_tool_capability;

        // Non-CLI shell with no write keyword — still unknown side-effects.
        let echo = serde_json::json!({"command": "echo hello-world"});
        assert!(
            !derive_tool_capability("shell", &echo).read_only,
            "echo is not a proven CLI query"
        );
        assert!(
            mutating_write_fingerprint("shell", &echo).is_some(),
            "non-CLI shell must fingerprint → gate"
        );

        let curl = serde_json::json!({
            "command": "curl -X POST https://example.com/hook -d '{}'"
        });
        assert!(mutating_write_fingerprint("shell", &curl).is_some());

        // Compound: proven RO + mutation → gated mutation.
        let compound_mut = serde_json::json!({
            "command": "neomind device list; neomind device delete cam-1"
        });
        assert!(!derive_tool_capability("shell", &compound_mut).read_only);
        assert!(mutating_write_fingerprint("shell", &compound_mut).is_some());

        // Compound: RO + non-CLI → unknown side-effects, still gated.
        let compound_unk = serde_json::json!({
            "command": "neomind device list; echo done"
        });
        let cap = derive_tool_capability("shell", &compound_unk);
        assert!(!cap.read_only);
        assert!(
            matches!(
                cap.retry_policy,
                crate::agent::task_continuity::RetryPolicy::Never
            ),
            "mixed CLI+shell must be unknown-sensitive"
        );
        assert!(mutating_write_fingerprint("shell", &compound_unk).is_some());

        // Unparseable neomind → unknown, gated (not keyword-RO).
        let bogus = serde_json::json!({"command": "neomind totally-bogus list"});
        assert!(!derive_tool_capability("shell", &bogus).read_only);
        assert!(mutating_write_fingerprint("shell", &bogus).is_some());

        // Unknown extension-shaped tool name → gated.
        let ext = serde_json::json!({"action": "run", "payload": {}});
        assert!(
            mutating_write_fingerprint("my_custom_extension_tool", &ext).is_some(),
            "unknown tools must not skip the write gate"
        );
    }

    #[test]
    fn fingerprint_ignores_json_object_key_order() {
        // preserve_order: insertion order survives to_string unless we sort.
        let a = serde_json::json!({"agent_id": "A", "param": "x"});
        let b = serde_json::json!({"param": "x", "agent_id": "A"});
        assert_ne!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap(),
            "precondition: preserve_order yields different strings"
        );
        let fp_a = mutating_write_fingerprint("run_now", &a).expect("a");
        let fp_b = mutating_write_fingerprint("run_now", &b).expect("b");
        assert_eq!(fp_a, fp_b, "reordered keys must share one fingerprint");

        let nested_a = serde_json::json!({
            "device_id": "D",
            "params": {"z": 1, "a": 2}
        });
        let nested_b = serde_json::json!({
            "params": {"a": 2, "z": 1},
            "device_id": "D"
        });
        let fp_na = mutating_write_fingerprint("device_command", &nested_a).expect("na");
        let fp_nb = mutating_write_fingerprint("device_command", &nested_b).expect("nb");
        assert_eq!(
            fp_na, fp_nb,
            "nested key order must not diverge fingerprints"
        );
    }

    #[test]
    fn new_operation_flag_excluded_from_business_fingerprint() {
        let base = serde_json::json!({"agent_id": "A"});
        let mut with_flag = base.clone();
        with_flag.as_object_mut().unwrap().insert(
            crate::agent::task_continuity::NEW_OPERATION_ARG.into(),
            serde_json::json!(true),
        );
        let fp1 = mutating_write_fingerprint("run_now", &base).expect("run_now");
        let fp2 = mutating_write_fingerprint("run_now", &with_flag).expect("run_now+flag");
        assert_eq!(
            fp1, fp2,
            "_new_operation must not enter the business fingerprint"
        );

        // Structured device control maps before fingerprinting.
        let alias = serde_json::json!({
            "action": "control",
            "device_id": "D",
            "command": "render_text",
            "param": "text=weather"
        });
        let fp = mutating_write_fingerprint("device", &alias).expect("device control is a write");
        assert!(fp.contains("control"), "fp={fp}");
        assert!(
            !fp.contains("_new_operation"),
            "lifecycle flag must not appear in fp: {fp}"
        );

        // CLI alias + flag must share the same business fingerprint (strip
        // before build_cli_command — otherwise `--_new_operation` is baked in).
        let mut alias_flag = alias.clone();
        alias_flag.as_object_mut().unwrap().insert(
            crate::agent::task_continuity::NEW_OPERATION_ARG.into(),
            serde_json::json!(true),
        );
        let fp_flag = mutating_write_fingerprint("device", &alias_flag).expect("flagged write");
        assert_eq!(
            fp, fp_flag,
            "CLI alias fingerprint must ignore _new_operation; plain={fp} flagged={fp_flag}"
        );
    }

    #[test]
    fn device_list_read_query_is_not_write_leased() {
        let list = serde_json::json!({"action": "list"});
        assert!(
            crate::agent::task_continuity::derive_tool_capability("device", &list).read_only,
            "device list is read_only"
        );
        assert!(
            mutating_write_fingerprint("device", &list).is_none(),
            "read/list must not enter the write lease"
        );
        // Idempotent: already-canonical shell form must stay read_only.
        let canon = crate::agent::task_continuity::canonicalize_tool_arguments("device", &list);
        assert!(
            crate::agent::task_continuity::derive_tool_capability("device", &canon).read_only,
            "re-deriving on canonical args must not flip list→control"
        );
        assert!(mutating_write_fingerprint("device", &canon).is_none());
    }

    #[test]
    fn shell_write_fingerprint_preserves_command_case_and_arg_values() {
        let mixed = serde_json::json!({
            "command": "neomind device control DeviceA --action Reboot --note 'KeepCASE'"
        });
        let fp = mutating_write_fingerprint("shell", &mixed).expect("mutation");
        assert!(fp.contains("DeviceA"), "device id case must survive: {fp}");
        assert!(
            fp.contains("Reboot") || fp.contains("KeepCASE"),
            "arg values must not be lowercased away: {fp}"
        );
        assert!(
            !fp.contains("devicea --action reboot"),
            "must not fingerprint the fully-lowercased command: {fp}"
        );
        // Detection still case-insensitive.
        let upper = serde_json::json!({
            "command": "NEOMIND DEVICE CONTROL sensor-001 --action reboot"
        });
        assert!(mutating_write_fingerprint("shell", &upper).is_some());
    }

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
        // Keyword detector must not false-positive on bounded/substring traps.
        assert!(
            !is_mutation_command("neomind agent list --status starting"),
            "`starting` must not match bounded ` start ` pattern"
        );
        // Proven CLI queries (parseable) stay read-only for the write gate.
        for cmd in [
            "neomind push list",
            "neomind push get webhook-1",
            "neomind push stats",
            "neomind connector subscriptions",
            "neomind agent list",
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
        let directive = build_unkept_promise_directive(
            "neomind dashboard add-components",
            2,
            "I will run the command now.",
        );
        assert!(directive.contains("STATED-INTENT MISMATCH"), "{directive}");
        assert!(directive.contains("add-components"), "{directive}");
        // No mixed-language instruction: the directive is English-only for
        // an English narration...
        assert!(
            !directive
                .chars()
                .any(|c| matches!(c, '\u{4e00}'..='\u{9fff}')),
            "no CJK in the English directive: {directive}"
        );
        // ...and Chinese-only for a Chinese one.
        let zh = build_unkept_promise_directive(
            "neomind dashboard add-components",
            2,
            "我现在就运行这个命令。",
        );
        assert!(zh.contains("add-components"), "{zh}");
        assert!(zh.contains("轮回复"), "{zh}");
        assert!(
            !zh.contains("STATED-INTENT MISMATCH"),
            "no English sentence in the Chinese directive: {zh}"
        );
    }

    /// P1-2: canonical fingerprint is the single source of truth shared across
    /// `ToolResultCache::make_key`, read-only reuse / loop detection, and
    /// `mutating_write_fingerprint` (write ledger).
    #[test]
    fn canonical_fingerprint_unifies_read_cache_and_write_ledger() {
        use super::super::cache::ToolResultCache;

        // 1) Read-only tool with reordered nested JSON keys shares one key.
        let ro_a = serde_json::json!({
            "image": "img.png",
            "options": {"z": 1, "a": 2}
        });
        let ro_b = serde_json::json!({
            "options": {"a": 2, "z": 1},
            "image": "img.png"
        });
        let fp_ro_a = canonical_tool_call_fingerprint("vision", &ro_a);
        let fp_ro_b = canonical_tool_call_fingerprint("vision", &ro_b);
        assert_eq!(fp_ro_a, fp_ro_b);
        assert_eq!(ToolResultCache::make_key("vision", &ro_a), fp_ro_a);
        assert_eq!(ToolResultCache::make_key("vision", &ro_b), fp_ro_b);

        // 2) Mutating shell call: CLI flag form, structured params form, and
        //    `_new_operation` form all collapse to the exact same canonical
        //    fingerprint as `mutating_write_fingerprint` and `ToolResultCache::make_key`.
        let mut_flag = serde_json::json!({
            "command": "neomind dashboard add-components d1 --components '[{\"type\":\"card\",\"id\":\"c1\"}]'"
        });
        let mut_struct = serde_json::json!({
            "command": "neomind dashboard add-components d1",
            "params": [{"id": "c1", "type": "card"}],
            "_new_operation": true
        });
        let fp_flag = canonical_tool_call_fingerprint("shell", &mut_flag);
        let fp_struct = canonical_tool_call_fingerprint("shell", &mut_struct);
        assert_eq!(fp_flag, fp_struct);
        assert_eq!(
            mutating_write_fingerprint("shell", &mut_flag).as_deref(),
            Some(fp_flag.as_str())
        );
        assert_eq!(
            mutating_write_fingerprint("shell", &mut_struct).as_deref(),
            Some(fp_struct.as_str())
        );
        assert_eq!(ToolResultCache::make_key("shell", &mut_flag), fp_flag);
        assert_eq!(ToolResultCache::make_key("shell", &mut_struct), fp_struct);
    }

    /// P2-1 anti-drift check: every `DomainCommandContract` across the ten core
    /// domains must stay aligned with `derive_tool_capability_resolved`,
    /// `mutating_write_fingerprint`, and structured-vs-flag canonical equivalence.
    #[test]
    fn core_domain_contracts_align_with_agent_capability_and_fingerprints() {
        use neomind_core::command_contract::{core_domain_contracts, DomainCommandSideEffect};

        for contract in core_domain_contracts() {
            let ex = contract.shell_example_value();
            let cap = crate::agent::task_continuity::derive_tool_capability_resolved("shell", &ex);
            let expected_ro = contract.side_effect == DomainCommandSideEffect::ReadOnly;
            assert_eq!(
                cap.read_only, expected_ro,
                "derive_tool_capability_resolved disagreed with contract for `{} {}`",
                contract.domain, contract.action
            );

            let fp_structured = mutating_write_fingerprint("shell", &ex);
            assert_eq!(
                fp_structured.is_some(),
                !expected_ro,
                "mutating_write_fingerprint presence mismatch for `{} {}`",
                contract.domain,
                contract.action
            );

            // When a primary JSON flag exists, passing the same payload via
            // CLI flag string vs shell `params` must produce the exact same
            // canonical write fingerprint and pass ShellTool validation.
            if let (Some(spec), Some(params)) = (contract.primary_json, ex.get("params")) {
                crate::toolkit::shell::validate_extension_params_sources(&ex).unwrap_or_else(|e| {
                    panic!(
                        "validate_extension_params_sources rejected contract example for `{} {}`: {e}",
                        contract.domain, contract.action
                    )
                });
                let base_cmd = ex["command"].as_str().unwrap();
                let flag_cmd = format!(
                    "{base_cmd} {} '{}'",
                    spec.flag,
                    params.to_string().replace('\'', "'\\''")
                );
                let flag_args = serde_json::json!({ "command": flag_cmd });
                let fp_flag = mutating_write_fingerprint("shell", &flag_args);
                assert_eq!(
                    fp_structured, fp_flag,
                    "structured params vs CLI flag fingerprint mismatch for `{} {}`",
                    contract.domain, contract.action
                );
            }
        }
    }

    #[test]
    fn agent_status_question_matches_eval_wording() {
        assert!(looks_like_agent_status_question("冷库现在还好吗?"));
        assert!(looks_like_agent_status_question(
            "Is the cold room OK right now?"
        ));
        assert!(looks_like_agent_status_question("Is fridge-1 okay?"));
        assert!(!looks_like_agent_status_question("列出所有设备"));
        assert!(!looks_like_agent_status_question(
            "How do I create a device?"
        ));
        assert!(!looks_like_agent_status_question("重新检查一下冷库还好吗"));
        // Cross-domain negatives (must not hijack)
        assert!(!looks_like_agent_status_question(
            "查询 weather-forecast 扩展的当前状态"
        ));
        assert!(!looks_like_agent_status_question(
            "What is the current status of connector mqtt-1?"
        ));
        assert!(!looks_like_agent_status_question(
            "传感器 sensor-001 当前状态正常吗？"
        ));
        assert!(!looks_like_agent_status_question("系统服务现在正常吗？"));
        // "is" word-boundary: "This … ok" must not match
        assert!(!looks_like_agent_status_question(
            "This dashboard layout looks ok, add a gauge widget"
        ));
    }

    #[test]
    fn guided_mqtt_onboarding_requires_create_action() {
        let en = "I want to connect my temperature sensor to NeoMind over MQTT. Can you guide me through setting it up?";
        assert!(user_message_requires_action(en));
        assert!(looks_like_guided_device_onboarding(en));
        assert_eq!(extract_action_hint(en), "neomind device create");
        let zh = "我想把我的温度传感器通过 MQTT 接入 NeoMind。你能引导我配置吗？";
        assert!(user_message_requires_action(zh));
        assert!(looks_like_guided_device_onboarding(zh));
        assert_eq!(extract_action_hint(zh), "neomind device create");
        assert!(
            build_device_onboarding_redirect(en, &["neomind device types list"])
                .unwrap()
                .contains("device create")
        );
        assert!(build_device_onboarding_redirect(
            en,
            &["neomind device create --name t --device-type temp_sensor --adapter-type mqtt"]
        )
        .is_none());
    }

    #[test]
    fn guided_device_onboarding_rejects_cross_domain_connect() {
        assert!(!looks_like_guided_device_onboarding(
            "帮我把外部 MQTT Broker 192.168.1.50 接入系统"
        ));
        assert!(!looks_like_guided_device_onboarding(
            "帮我接入钉钉 webhook 通知渠道"
        ));
        assert!(!looks_like_guided_device_onboarding(
            "Connect a webhook channel to Slack"
        ));
        assert!(!looks_like_guided_device_onboarding(
            "帮我接入 DeepSeek 大模型"
        ));
        assert!(!looks_like_guided_device_onboarding(
            "Can you walk me through creating a temperature alert rule and set it up?"
        ));
        // Mutation exemption: connector create already succeeded
        assert!(build_device_onboarding_redirect(
            "I want to connect my temperature sensor to NeoMind over MQTT. Can you guide me through setting it up?",
            &["neomind connector create --name ext --host 1.2.3.4"],
        )
        .is_none());
    }

    #[test]
    fn agent_conclusion_redirect_fires_on_device_list_detour() {
        let zh = build_agent_conclusion_redirect("冷库现在还好吗?", &["neomind device list"])
            .expect("zh status + device list must redirect");
        assert!(zh.contains("latest-execution"), "{zh}");
        assert!(zh.contains("agent list"), "{zh}");

        let en = build_agent_conclusion_redirect(
            "Is the cold room OK right now?",
            &["neomind system info", "neomind device list"],
        )
        .expect("en status + detour must redirect");
        assert!(en.contains("latest-execution"), "{en}");

        assert!(build_agent_conclusion_redirect(
            "Is the cold room OK right now?",
            &["neomind agent latest-execution coldroom-watch"],
        )
        .is_none());
        assert!(build_agent_conclusion_redirect("列出所有设备", &[]).is_none());
        assert!(build_agent_conclusion_redirect(
            "查询 weather-forecast 扩展的当前状态",
            &["neomind extension status weather-forecast"],
        )
        .is_none());
    }

    #[test]
    fn empty_agent_list_exempts_conclusion_redirect() {
        let cmds = ["neomind agent list"];
        let empty = [(
            "shell".to_string(),
            r#"{"success":true,"data":{"total":0,"agents":[]}}"#.to_string(),
        )];
        assert!(
            build_agent_conclusion_redirect_with_results("冷库现在还好吗?", &cmds, &empty,)
                .is_none()
        );

        // Non-empty list without latest-execution still redirects.
        let populated = [(
            "shell".to_string(),
            r#"{"success":true,"data":{"total":1,"agents":[{"id":"a1"}]}}"#.to_string(),
        )];
        assert!(
            build_agent_conclusion_redirect_with_results("冷库现在还好吗?", &cmds, &populated,)
                .is_some()
        );
    }
}
