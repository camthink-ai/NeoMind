//! `custom:task-understanding` — the knowledge file that carries an agent's
//! task definition next to what the agent learns on the job.
//!
//! The file is generated with a `## Mission` section that mirrors
//! `AiAgent::user_prompt` verbatim, and every run inlines the whole file into
//! the system prompt. The moment the operator edits the prompt, that copy is
//! stale and the model keeps receiving the old mission: on T4 the sentence
//! "用 vision 工具分析该图片" survived a prompt edit for two days because the
//! edit never reached the file. An agent can also replace the file with a
//! per-run observation ("无图像数据路径，需设备日志搜索"), leaving it with
//! learned notes and no task at all.
//!
//! [`task_understanding_repair`] is the one place that puts the section right.
//! People edit this file by hand as well, so it writes only what it can
//! prove: where the old mission's extent is not known, the file is left
//! alone and the repair serves the prompt only.

/// Knowledge-file name of the auto-generated task definition.
pub const TASK_UNDERSTANDING_FILE: &str = "task-understanding";

/// Headings the stock templates put right after `## Mission`.
const MISSION_SUCCESSORS: [&str; 2] = ["## Bound Resources", "## Resources"];

/// A task-understanding file repaired so its Mission section carries the
/// current task prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissionRepair {
    /// The file as a run should inline it — the stored content when nothing
    /// was wrong with it.
    pub content: String,
    /// Whether `content` may replace the stored file.
    ///
    /// True only for a repair that removes nothing but text known to be the
    /// old mission (or adds a missing section). False when the extent of the
    /// old mission is not known: what sits under the heading may be the
    /// operator's own notes, so the repair is for the prompt alone.
    pub store: bool,
}

/// Body of the file's `## Mission` section up to the next heading of level
/// 1–2 or `---` rule, or `None` when it has none.
///
/// For display and tests, and best effort: the section is cut at the first
/// such line even when the prompt itself contains one. No repair depends on
/// this cut.
pub fn task_understanding_mission(content: &str) -> Option<&str> {
    let start = mission_body_start(content)?;
    Some(content[start..next_section_start(content, start).unwrap_or(content.len())].trim())
}

/// Bring the file's `## Mission` section in line with `mission`.
///
/// `previous_mission` is the prompt before an edit, when the caller knows
/// it. Three outcomes:
///
/// - **The previous prompt is known and the section starts with it**: that
///   text is the old mission, and only it moves — every byte after it stays
///   where it was, be it a section the operator filed between Mission and
///   Resources or notes written under the mission. Stored.
/// - **In sync** — the section starts with `mission`: the content is returned
///   unchanged.
/// - **Anything else** (no previous prompt, or the section no longer starts
///   with it): the old mission cannot be told from notes written around it.
///   `content` then shows `mission` in the section's place, up to the
///   template's next heading, for the prompt only; `store` is false and the
///   file on disk is the operator's to fix.
///
/// A file without a Mission section gets one on top of content that is
/// otherwise untouched — the task comes back while whatever the agent wrote
/// below it survives.
pub fn task_understanding_repair(
    content: &str,
    mission: &str,
    previous_mission: Option<&str>,
) -> MissionRepair {
    let mission = mission.trim();
    let Some(body_start) = mission_body_start(content) else {
        return MissionRepair {
            content: with_missing_mission(content, mission),
            store: true,
        };
    };
    let body = &content[body_start..];

    // Checked before "in sync": a shortened prompt keeps its first line, so
    // the new mission would match while the rest of the old one stayed under
    // the heading.
    if let Some(after_old) = previous_mission
        .map(str::trim)
        .and_then(|old| mission_ends_a_line(body, old))
    {
        let mut out = String::with_capacity(content.len() + mission.len());
        out.push_str(&content[..body_start]);
        out.push_str(mission);
        out.push_str(&content[body_start + after_old..]);
        return MissionRepair {
            content: out,
            store: true,
        };
    }

    if mission_ends_a_line(body, mission).is_some() {
        return MissionRepair {
            content: content.to_string(),
            store: false,
        };
    }

    MissionRepair {
        content: splice(
            content,
            body_start,
            mission_view_end(content, body_start),
            mission,
        ),
        store: false,
    }
}

/// The file with a Mission section carrying `mission` put on top. Nothing is
/// removed.
fn with_missing_mission(content: &str, mission: &str) -> String {
    let mut out = String::with_capacity(content.len() + mission.len() + 32);
    out.push_str("# Task Understanding\n\n## Mission\n");
    out.push_str(mission);
    out.push_str("\n\n");
    out.push_str(content.trim_start());
    out
}

/// Replace `content[start..end]` with `mission`, keeping one blank line
/// before what follows.
fn splice(content: &str, start: usize, end: usize, mission: &str) -> String {
    let mut out = String::with_capacity(content.len() + mission.len());
    out.push_str(&content[..start]);
    out.push_str(mission);
    if end < content.len() {
        out.push_str("\n\n");
        out.push_str(&content[end..]);
    } else {
        out.push('\n');
    }
    out
}

/// Where `text` ends inside the section body, when the body starts with it
/// and it ends the body or its line: `Some(text.len())`. `None` when the body
/// carries something else — more text glued to the last character of `text`
/// is a longer line, not the end of it.
fn mission_ends_a_line(body: &str, text: &str) -> Option<usize> {
    if !body.starts_with(text) {
        return None;
    }
    let rest = &body[text.len()..];
    (rest.is_empty() || rest.starts_with('\n') || rest.starts_with("\r\n")).then_some(text.len())
}

/// End of what stands in for the mission, measured from `from`: the heading
/// the stock template puts after Mission, else the next heading of level 1–2
/// or `---` rule, else the end of the file.
///
/// For the prompt-only repair. In the stock layout that keeps a prompt with
/// headings of its own from leaving its tail behind; a layout rebuilt without
/// those headings falls back to the next heading.
fn mission_view_end(content: &str, from: usize) -> usize {
    next_line_starting_with(content, from, &MISSION_SUCCESSORS)
        .or_else(|| next_section_start(content, from))
        .unwrap_or(content.len())
}

/// Byte offset just past the `## Mission` heading line.
fn mission_body_start(content: &str) -> Option<usize> {
    lines_from(content, 0)
        .find(|(_, line)| line.trim_end_matches(['\r', '\n']).trim_end() == "## Mission")
        .map(|(offset, line)| offset + line.len())
}

/// Byte offset of the first line at `from` or later that opens a section.
fn next_section_start(content: &str, from: usize) -> Option<usize> {
    lines_from(content, from)
        .find(|(_, line)| starts_section(line))
        .map(|(offset, _)| offset)
}

/// Byte offset of the first line at `from` or later that is one of
/// `headings` (trailing whitespace aside).
fn next_line_starting_with(content: &str, from: usize, headings: &[&str]) -> Option<usize> {
    lines_from(content, from)
        .find(|(_, line)| headings.contains(&line.trim_end_matches(['\r', '\n']).trim_end()))
        .map(|(offset, _)| offset)
}

/// A heading of level 1–2 or a `---` rule.
fn starts_section(line: &str) -> bool {
    let line = line.trim_end_matches(['\r', '\n']);
    line == "---" || line.starts_with("# ") || line.starts_with("## ")
}

/// `(offset, line)` for each line from `from` on. `from` sits on a line
/// boundary.
fn lines_from(content: &str, from: usize) -> impl Iterator<Item = (usize, &str)> {
    content[from..]
        .split_inclusive('\n')
        .scan(from, |offset, line| {
            let start = *offset;
            *offset += line.len();
            Some((start, line))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const API_TEMPLATE: &str =
        "# Task Understanding\n\n## Identity & Role\nYou are an IoT agent.\n\n\
        ## Mission\nOLD MISSION\n\n## Bound Resources\nNone (free mode)\n\n## Schedule\n\
        Event-driven\n\n## Notes\nlearned notes\n";
    const EXECUTOR_TEMPLATE: &str = "# Task Understanding\n\n## Role\nagent\n\n## Mission\n\
        OLD MISSION\n\n## Resources\nNone\n\n## Schedule\nEvent-driven\n\n---\n\
        Update this file as you discover thresholds.\n";
    /// A file the operator reworked: their own section filed between the
    /// mission and the template's Resources.
    const OPERATOR_FILE: &str =
        "# Task Understanding\n\n## Mission\nOLD MISSION\n\n## Operator Notes\nKEEP-ME\n\n\
        ## Resources\nNone\n";
    /// A prompt with headings of its own.
    const HEADED_PROMPT: &str =
        "每天巡检。\n## 判定标准\n用 vision 工具分析该图片\n\n---\n周日休息";

    fn with_mission(template: &str, mission: &str) -> String {
        template.replacen("OLD MISSION", mission, 1)
    }

    #[test]
    fn reads_the_mission_section() {
        assert_eq!(
            task_understanding_mission(API_TEMPLATE),
            Some("OLD MISSION")
        );
        assert_eq!(
            task_understanding_mission(EXECUTOR_TEMPLATE),
            Some("OLD MISSION")
        );
        assert_eq!(
            task_understanding_mission("## Mission\nonly mission"),
            Some("only mission")
        );
        assert_eq!(task_understanding_mission("## Mission"), Some(""));
        assert_eq!(
            task_understanding_mission(OPERATOR_FILE),
            Some("OLD MISSION"),
            "cut at the operator's next heading"
        );
        assert_eq!(
            task_understanding_mission("free notes\n## Observations\nx"),
            None
        );
        assert_eq!(task_understanding_mission(""), None);
    }

    #[test]
    fn an_in_sync_file_is_returned_untouched() {
        for previous in [None, Some("an older prompt")] {
            let repair = task_understanding_repair(API_TEMPLATE, "  OLD MISSION \n", previous);
            assert_eq!(
                repair,
                MissionRepair {
                    content: API_TEMPLATE.to_string(),
                    store: false,
                }
            );
        }
        // A prompt with headings of its own, mirrored whole.
        let file = with_mission(API_TEMPLATE, HEADED_PROMPT);
        let repair = task_understanding_repair(&file, HEADED_PROMPT, None);
        assert_eq!(repair.content, file);
        assert!(!repair.store);
    }

    /// A prompt edit. The previous prompt is what the section starts with, so
    /// exactly that text moves — and the operator's section between Mission
    /// and Resources, or anything else, stays byte for byte.
    #[test]
    fn an_edit_replaces_exactly_the_previous_prompt() {
        let repair = task_understanding_repair(OPERATOR_FILE, "NEW MISSION", Some("OLD MISSION"));

        assert!(repair.store, "a known old mission may be replaced on disk");
        assert_eq!(
            repair.content,
            OPERATOR_FILE.replacen("OLD MISSION", "NEW MISSION", 1)
        );
        assert!(repair
            .content
            .contains("## Operator Notes\nKEEP-ME\n\n## Resources"));
    }

    /// The same edit for a prompt with headings of its own: matching the
    /// known text does not stop at the first heading inside it. The new
    /// mission is also the old prompt's first line — the in-sync check alone
    /// would have let the rest of the old prompt stand.
    #[test]
    fn an_edit_handles_a_previous_prompt_with_headings() {
        let file = with_mission(API_TEMPLATE, HEADED_PROMPT);
        let repair = task_understanding_repair(&file, "每天巡检。", Some(HEADED_PROMPT));

        assert!(repair.store);
        assert_eq!(
            repair.content,
            API_TEMPLATE.replacen("OLD MISSION", "每天巡检。", 1),
            "{}",
            repair.content
        );
        assert!(!repair.content.contains("vision"));
        assert!(!repair.content.contains("## 判定标准"));
        assert!(!repair.content.contains("周日休息"));
    }

    /// Both at once, in the operator's file.
    #[test]
    fn an_edit_keeps_the_operators_section_next_to_a_headed_prompt() {
        let file = with_mission(OPERATOR_FILE, HEADED_PROMPT);
        let repair = task_understanding_repair(&file, "每天巡检。", Some(HEADED_PROMPT));

        assert!(repair.store);
        assert_eq!(
            repair.content,
            OPERATOR_FILE.replacen("OLD MISSION", "每天巡检。", 1)
        );
        assert!(repair.content.contains("KEEP-ME"));
        assert!(!repair.content.contains("vision"));
    }

    /// The edit path does not guess: a mission that no longer starts with the
    /// previous prompt (edited by hand, or overwritten by the agent) is not
    /// rewritten on disk — what is under the heading may be the operator's.
    #[test]
    fn an_edit_leaves_a_mission_it_cannot_match_alone() {
        for file in [
            with_mission(OPERATOR_FILE, "OLD MISSION, trimmed by hand"),
            with_mission(OPERATOR_FILE, "用 vision 工具分析该图片"),
        ] {
            let repair = task_understanding_repair(&file, "NEW MISSION", Some("OLD MISSION"));

            assert!(
                !repair.store,
                "the file must not be rewritten: {}",
                repair.content
            );
            assert_ne!(repair.content, file);
            assert!(repair.content.contains("## Mission\nNEW MISSION"));
        }
    }

    /// Before a run the previous prompt is not known. The prompt then shows
    /// the task in the section's place, and the file is left to its owner.
    #[test]
    fn a_stale_mission_is_repaired_for_the_prompt_only() {
        for file in [
            with_mission(API_TEMPLATE, "OLD MISSION 用 vision 工具分析该图片"),
            with_mission(API_TEMPLATE, HEADED_PROMPT),
            OPERATOR_FILE.to_string(),
        ] {
            // Not the first line of any old text: the per-run check has no
            // previous prompt and reads such a first line as in sync.
            let repair = task_understanding_repair(&file, "每周巡检。", None);

            assert!(!repair.store);
            assert_ne!(repair.content, file);
            assert!(
                repair.content.contains("## Mission\n每周巡检。"),
                "{}",
                repair.content
            );
            assert!(!repair.content.contains("vision"), "{}", repair.content);
            assert!(
                !repair.content.contains("## 判定标准"),
                "{}",
                repair.content
            );
            assert!(
                repair
                    .content
                    .contains("## Bound Resources\nNone (free mode)")
                    || repair.content.contains("## Resources\nNone"),
                "{}",
                repair.content
            );
        }
        // Without the template's headings the section ends at the next one.
        let custom = "## Mission\nold task\nmore of it\n\n## Observations\nnote\n";
        let repair = task_understanding_repair(custom, "new task", None);
        assert_eq!(
            repair.content,
            "## Mission\nnew task\n\n## Observations\nnote\n"
        );
    }

    /// An operator may write under the mission without a heading. That is
    /// theirs too: an in-sync file is never touched for it.
    #[test]
    fn notes_under_an_in_sync_mission_are_left_alone() {
        let file = "## Mission\nM\n\nwritten by hand, no heading\n\n## Bound Resources\nNone\n";
        let repair = task_understanding_repair(file, "M", None);
        assert_eq!(repair.content, file);
        assert!(!repair.store);
    }

    #[test]
    fn restores_a_missing_mission_above_the_agents_notes() {
        // The T4 200366 shape: the agent replaced its task with one
        // observation. The task comes back on top; the note is kept.
        let corrupted =
            "# NE301PIR-200366 任务理解\n## 图像数据路径\n- 无图像数据路径，需设备日志搜索\n";
        for previous in [None, Some("the prompt before the edit")] {
            let repair = task_understanding_repair(corrupted, "只在工作日告警", previous);

            assert!(
                repair.store,
                "a missing section is added; nothing is removed"
            );
            assert_eq!(
                repair.content,
                format!("# Task Understanding\n\n## Mission\n只在工作日告警\n\n{corrupted}")
            );
            assert_eq!(
                task_understanding_mission(&repair.content),
                Some("只在工作日告警")
            );
        }
    }

    #[test]
    fn empty_file_gets_just_the_mission() {
        // read_agent_custom_file yields "" for a missing file.
        let repair = task_understanding_repair("", "task", None);
        assert!(repair.store);
        assert_eq!(
            repair.content,
            "# Task Understanding\n\n## Mission\ntask\n\n"
        );
    }

    #[test]
    fn mission_as_last_section() {
        let repair = task_understanding_repair("## Role\nagent\n\n## Mission\nold", "new", None);
        assert!(!repair.store);
        assert_eq!(repair.content, "## Role\nagent\n\n## Mission\nnew\n");
    }

    /// The T4 failure in its simplest form: a line removed from the prompt
    /// goes from the file too, and nothing else moves.
    #[test]
    fn a_shortened_prompt_takes_its_old_lines_with_it() {
        let old = "巡检东坪山。\n用 vision 工具分析该图片";
        let file = with_mission(API_TEMPLATE, old);

        let repair = task_understanding_repair(&file, "巡检东坪山。", Some(old));

        assert!(repair.store);
        assert_eq!(
            repair.content,
            API_TEMPLATE.replacen("OLD MISSION", "巡检东坪山。", 1)
        );
        // Stable from here on.
        assert_eq!(
            task_understanding_repair(&repair.content, "巡检东坪山。", None).content,
            repair.content
        );
    }

    /// More text glued to the last character is a longer line, not the
    /// mission plus a note: it is not taken for in sync.
    #[test]
    fn a_longer_line_is_not_mistaken_for_the_mission() {
        let file = with_mission(API_TEMPLATE, "巡检东坪山，顺带看水位");
        let repair = task_understanding_repair(&file, "巡检东坪山", None);
        assert!(!repair.store);
        assert!(!repair.content.contains("水位"), "{}", repair.content);
    }

    /// A prompt edit where the file was already behind by more than one
    /// edit: the previous prompt is not what the section holds, so the file
    /// stays and the next run shows the task from the prompt.
    #[test]
    fn a_file_behind_by_more_than_one_edit_is_kept() {
        let file = with_mission(OPERATOR_FILE, "two edits ago");
        let repair = task_understanding_repair(&file, "now", Some("one edit ago"));

        assert!(!repair.store);
        assert!(repair.content.contains("## Mission\nnow"));
        assert!(!repair.content.contains("two edits ago"));
    }
}
