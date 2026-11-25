//! Narration-without-action guard, the "plan only, don't execute" check and
//! the one-shot narration nudge prompt.
//!
//! Moved verbatim from `neomind-agent/src/agent/context.rs` (M0 extraction);
//! that path re-exports from here.

/// Phrases where the model PROMISES a tool action in words. Matched against
/// a text-only (no tool call) LLM round to detect the "narration collapse":
/// the model writes "我现在使用 image_edit 工具…" and the loop would
/// otherwise end the turn with that promise as the final answer (observed:
/// 0.3s turn end while the user waits for a watermarked image that never
/// comes). English patterns are lowercase — the text is lowercased before
/// matching, which leaves the Chinese patterns untouched.
///
/// Only matched against the HEAD of the reply (see [`PROMISE_HEAD_CHARS`]):
/// narration collapse LEADS with the promise; politeness closings ("随时
/// 告诉我，我来帮你实现！") live at the tail of a complete answer.
const ACTION_PROMISE_PATTERNS: &[&str] = &[
    // Chinese
    "我将使用",
    "我会使用",
    "我将调用",
    "我会调用",
    "我将执行",
    "我会执行",
    "让我使用",
    "让我调用",
    "让我执行",
    "接下来我将",
    "现在我将",
    "现在我会",
    "我将通过",
    "我会通过",
    "接着我将",
    "我将先",
    "我会先",
    "我来使用",
    "我来调用",
    // "现在 + action verb" — covers the observed "我现在使用 X 工具…" form
    "现在使用",
    "现在调用",
    "现在执行",
    "现在运行",
    // 2026-10-08 widget-task session: the small model ended FOUR turns with
    // promise-narrations none of the patterns above matched — "现在让我重新
    // 安装…到平台：", "现在最后一步：将气象站组件添加到…", "让我修正命令
    // 语法：", "现在我来将气象站组件添加到…中：". Each read as a task
    // "interruption" to the user, who had to nudge five times.
    "现在让我",
    "让我重新",
    "让我继续",
    "让我完成",
    "让我修正",
    "让我修复",
    "让我安装",
    "让我创建",
    "让我添加",
    "让我配置",
    "让我检查",
    "让我验证",
    "让我试试",
    "我来将",
    "现在我来",
    "接下来我",
    "下一步我",
    "最后一步",
    // 2026-10-19 incident session tail (reproduced via conversation replay):
    // after the dashboard was built, THREE consecutive turns answered with
    // promise-only replies none of the patterns above matched — 「我来检查
    // 仪表盘和组件的当前状态，确认是否正确绑定。」「我来帮您继续处理…
    // 首先让我了解仪表盘的可用操作…」. The table had 让我检查 but not
    // 我来检查; the nudge never fired, the user pressed 继续 twice and got
    // the same promise again — this read as the task being "interrupted".
    "我来检查",
    "我来查看",
    "我来帮",
    "我来处理",
    "我来确认",
    "让我了解",
    "首先让我",
    // 2026-11-18 NM-250: foreshadow-only weather turns used 「我来获取 /
    // 我将查询 / 我会尝试…」shapes the older table missed — auto-continue
    // and the incomplete-end label share this detector, so both missed.
    "我将尝试",
    "我会尝试",
    "我来尝试",
    "让我尝试",
    "我将获取",
    "我会获取",
    "我来获取",
    "让我获取",
    "我将查询",
    "我会查询",
    "我来查询",
    "让我查询",
    "我将拉取",
    "我会拉取",
    "我来拉取",
    "让我拉取",
    // English
    "i will use",
    "i'll use",
    "i will call",
    "i'll call",
    "i will run",
    "i'll run",
    "i will invoke",
    "i'll invoke",
    "let me use",
    "let me call",
    "let me run",
    "let me check",
    "let me look",
    "let me inspect",
    "i will check",
    "i'll check",
    // 2026-11-18 NM-250 desktop: "I'll try fetching…" / "I'll fetch…" ended
    // the turn with tools still available — table had use/run/check only.
    "i will try",
    "i'll try",
    "i will fetch",
    "i'll fetch",
    "i will get",
    "i'll get",
    "let me try",
    "let me fetch",
    "let me get",
    "i'm going to use",
    "i'm going to call",
    "i'm going to run",
    "i'm going to try",
    "i'm going to fetch",
    "i'm going to get",
    // 2026-11-21 tip overnight + T1.3 retest deep-memory-transform: foreshadow
    // turns used list/read/delete/disable/re-enable/test/query/add shapes the
    // table missed — NM-250 continue never fired, soft-asserts failed.
    "i will list",
    "i'll list",
    "let me list",
    "i will read",
    "i'll read",
    "let me read",
    "i will re-read",
    "i'll re-read",
    "let me re-read",
    "i will delete",
    "i'll delete",
    "let me delete",
    "i will disable",
    "i'll disable",
    "let me disable",
    "i will enable",
    "i'll enable",
    "let me enable",
    "i will re-enable",
    "i'll re-enable",
    "let me re-enable",
    "i will test",
    "i'll test",
    "let me test",
    "i will query",
    "i'll query",
    "let me query",
    "i will add",
    "i'll add",
    "let me add",
    "i'm going to list",
    "i'm going to read",
    "i'm going to re-read",
    "i'm going to delete",
    "i'm going to disable",
    "i'm going to enable",
    "i'm going to re-enable",
    "i'm going to test",
    "i'm going to query",
    "i'm going to add",
    "我来列出",
    "我将列出",
    "我会列出",
    "让我列出",
    "我来读取",
    "我将读取",
    "我会读取",
    "让我读取",
    "我来删除",
    "我将删除",
    "我会删除",
    "让我删除",
    "我来禁用",
    "我将禁用",
    "我会禁用",
    "让我禁用",
    "我来启用",
    "我将启用",
    "我会启用",
    "让我启用",
    "我来重新启用",
    "我将重新启用",
    "我会重新启用",
    "让我重新启用",
    "我来测试",
    "我将测试",
    "我会测试",
    "让我测试",
    "我来查询",
    "我将查询",
    "我会查询",
    "让我查询",
    "我来添加",
    "我将添加",
    "我会添加",
    "让我添加",
];

/// Max narration length eligible for the nudge — a long, complete analysis
/// that merely contains "let me use" somewhere is a legitimate final answer.
const NARRATION_MAX_CHARS: usize = 500;

/// How deep into the reply a promise pattern may sit and still count. A real
/// narration collapse opens with the promise ("我来检查仪表盘…", "Let me
/// check…"); the same words at the tail of a complete answer are a politeness
/// closing. Live 2026-10-20: a full ~300-char capability-list answer ending
/// in "随时告诉我，我来帮你一步步实现！" was misclassified, the nudge round
/// ran, and the small model answered the nudge with a bare `[]` — replacing
/// the real answer in the UI. Position, not just presence, decides.
const PROMISE_HEAD_CHARS: usize = 80;

fn narration_has_promise_actor(text: &str) -> bool {
    let lower = text.to_lowercase();
    const ACTORS: &[&str] = &[
        "我",
        "咱",
        "让我",
        "我来",
        "我会",
        "我将",
        "我现在",
        "i ",
        "i'll",
        "i will",
        "i am going",
        "i'm going",
        "let me",
        "ok i",
        "okay i",
    ];
    ACTORS.iter().any(|a| lower.contains(a))
}

fn english_promise_pattern_hits(lower: &str, pattern: &str) -> bool {
    // ASCII patterns need leading + trailing word boundaries so "let me add"
    // does not match "Let me address…" / "I'll list" does not match
    // "I'll listen…". Scan every hit — a false-friend earlier in the head
    // must not hide a later real promise ("Let me address that — let me add…").
    lower.match_indices(pattern).any(|(idx, _)| {
        let start_ok = if idx == 0 {
            true
        } else {
            let b = lower.as_bytes()[idx - 1];
            !b.is_ascii_alphanumeric() && b != b'_'
        };
        if !start_ok {
            return false;
        }
        let end = idx + pattern.len();
        match lower.as_bytes().get(end) {
            None => true,
            Some(b) => !b.is_ascii_alphanumeric() && *b != b'_',
        }
    })
}

pub fn is_narration_without_action(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.chars().count() > NARRATION_MAX_CHARS {
        return false;
    }
    // Trailing-colon heuristic: ONLY when the short reply also carries a
    // first-person / promise actor. Bare times or tip lists ending in ":"
    // must not be labelled "no result".
    if (trimmed.ends_with('：') || trimmed.ends_with(':')) && narration_has_promise_actor(trimmed)
    {
        return true;
    }
    // Complete list summaries after a successful tool round: first line may
    // still open with "I'll list…" / "我来列出…" but the body already has
    // bullet items — do not foreshadow-continue those.
    if trimmed.contains('\n')
        && (trimmed.contains("\n- ")
            || trimmed.contains("\n* ")
            || trimmed
                .lines()
                .skip(1)
                .any(|l| l.trim().starts_with(|c: char| c.is_ascii_digit())))
        && trimmed.chars().count() > 40
    {
        return false;
    }
    let head: String = trimmed.chars().take(PROMISE_HEAD_CHARS).collect();
    let lower = head.to_lowercase();
    ACTION_PROMISE_PATTERNS.iter().any(|p| {
        if p.is_ascii() {
            english_promise_pattern_hits(&lower, p)
        } else {
            lower.contains(*p)
        }
    })
}

/// True when `text` contains Chinese text — the language selector for
/// model-facing guard prompts.
///
/// Any CJK ideograph counts: Chinese conversations routinely embed English
/// identifiers (`neomind dashboard add-components`), while English
/// conversations essentially never embed CJK, so "contains CJK" is the
/// reliable discriminator. One prompt language per turn matters: a prompt
/// that states the instruction in BOTH languages makes small models drift
/// into the language they read LAST (observed as English replies to Chinese
/// questions after the old bilingual nudge).
pub fn text_prefers_chinese(text: &str) -> bool {
    text.chars()
        .any(|c| matches!(c, '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}'))
}

/// Phrases with which a user explicitly asks for a PLAN or EXPLANATION only —
/// "先不要执行", "只告诉我怎么做", "don't run it, just tell me". When one is
/// present the narration guard is suppressed for the turn: a promise-shaped
/// reply IS the requested output, and nudging the model into executing would
/// contradict the user's instruction (and the progress-report label would
/// mislabel a perfectly good plan as a failed turn).
const NO_EXECUTION_PATTERNS: &[&str] = &[
    // Chinese
    "不要执行",
    "别执行",
    "不要运行",
    "别运行",
    "不要调用",
    "不要操作",
    "不要动手",
    "只告诉我",
    "只需要告诉我",
    "只说明",
    "仅说明",
    "只解释",
    "先说明",
    "先告诉我",
    // English
    "do not execute",
    "don't execute",
    "do not run",
    "don't run",
    "do not call",
    "don't call",
    "without executing",
    "just tell me",
    "only tell me",
    "just explain",
    "only explain",
    "don't actually",
    "do not actually",
    "no need to execute",
];

/// True when the user's message asks for a plan/explanation WITHOUT running
/// anything. Suppresses the narration nudge, the promise backstop, the
/// turn-end completeness note, and the list-only coercion for that turn.
pub fn user_requested_no_execution(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    NO_EXECUTION_PATTERNS.iter().any(|p| lower.contains(p))
}

/// The one-shot nudge injected when narration collapse is detected.
///
/// Decision-first: small models follow the FIRST imperative they see, so the
/// old prompt (which led with "output the tool calls as a JSON array") got
/// literal compliance — a bare `[]` — whenever no tool was actually needed
/// (live 2026-10-20: the complete capability-list answer was replaced by
/// `[]`). The branch order and the explicit empty-array ban exist so the
/// model must judge before it emits anything.
///
/// ONE language per call, chosen from the narration being corrected (see
/// [`narration_nudge_prompt`]) — never both at once.
const NARRATION_NUDGE_PROMPT_ZH: &str = "你上一条回复承诺了要执行操作，但没有发出任何工具调用——这不是最终答复。请先判断：\n\
1. 如果确实需要工具才能完成用户的需求，现在立即只输出工具调用 JSON 数组：[{\"name\":\"工具名\",\"arguments\":{...}}]，不要输出其他文字。\n\
2. 如果不需要任何工具，就不要输出 JSON 数组（绝对不要输出空数组 []），直接用文字给用户完整的最终答复。";

const NARRATION_NUDGE_PROMPT_EN: &str = "Your previous reply promised an action but emitted no tool call — that is not a final answer. Decide first:\n\
1. If a tool is genuinely required, output ONLY the tool-call JSON array now: [{\"name\":\"...\",\"arguments\":{...}}] — no other text.\n\
2. If no tool is needed, do NOT output a JSON array (never an empty array \"[]\") — give the user the complete final answer as plain text.";

/// Pick the nudge prompt in the same language as `language_hint` (the
/// narration that triggered the guard), so the model continues in the
/// conversation's language.
pub fn narration_nudge_prompt(language_hint: &str) -> &'static str {
    if text_prefers_chinese(language_hint) {
        NARRATION_NUDGE_PROMPT_ZH
    } else {
        NARRATION_NUDGE_PROMPT_EN
    }
}
