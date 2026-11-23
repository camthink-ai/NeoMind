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
//! Two entry points re-point the section, and both restore it when it is
//! missing: [`task_understanding_repoint_mission`] after a prompt edit, and
//! [`task_understanding_with_mission`] for the check before every run.

/// Knowledge-file name of the auto-generated task definition.
pub const TASK_UNDERSTANDING_FILE: &str = "task-understanding";

/// Headings the stock templates put right after `## Mission`. The Mission
/// section runs to the first of these.
const MISSION_SUCCESSORS: [&str; 2] = ["## Bound Resources", "## Resources"];

/// Body of the file's `## Mission` section, or `None` when it has none.
///
/// Best effort — for display and tests. A file that kept the stock layout is
/// read exactly, whatever the prompt contains; in a layout of the agent's
/// own the section ends at the next heading or `---` line, so a prompt that
/// itself contains one is cut there. Neither sync below depends on this.
pub fn task_understanding_mission(content: &str) -> Option<&str> {
    let start = mission_body_start(content)?;
    Some(content[start..mission_section_end(content, start)].trim())
}

/// The file with its `## Mission` section set to `mission`, for the check
/// before a run. An up-to-date file comes back unchanged.
///
/// Everything else in the file — identity, resources, the agent's appended
/// sections — is kept byte for byte. A file without a Mission section gets
/// one on top, so a task definition that was overwritten comes back while
/// the notes written below it survive.
///
/// The mission is a verbatim copy of the prompt, and a prompt may itself
/// contain markdown headings or `---` lines. So the section is measured from
/// the END of the current mission, not from its first heading-like line:
/// cutting it there read an in-sync file as stale on every run and appended
/// the prompt again each time. Unheaded text written under the mission goes
/// — the section is the task's, not a place for notes.
///
/// Not enough after a prompt edit: when the old mission keeps the new one as
/// its first lines and what was removed starts with a heading, that heading
/// is indistinguishable from the next section of the file. The caller that
/// knows the prompt changed uses [`task_understanding_repoint_mission`].
pub fn task_understanding_with_mission(content: &str, mission: &str) -> String {
    let mission = mission.trim();
    let Some(body_start) = mission_body_start(content) else {
        return with_missing_mission(content, mission);
    };
    if let Some(after) = mission_tops_section(&content[body_start..], mission) {
        let after = body_start + after;
        let next = next_section_start(content, after).unwrap_or(content.len());
        if content[after..next].trim().is_empty() {
            return content.to_string();
        }
        return splice(content, body_start, next, mission);
    }
    replace_mission(content, body_start, mission)
}

/// The file with its `## Mission` section replaced by `mission`, whether or
/// not the old one looks current. For a prompt edit.
///
/// The old mission's extent is not known — it may hold headings of its own —
/// so the section runs to the heading the stock template puts after Mission,
/// else to the first heading-like line, else to the end of the file.
pub fn task_understanding_repoint_mission(content: &str, mission: &str) -> String {
    let mission = mission.trim();
    let Some(body_start) = mission_body_start(content) else {
        return with_missing_mission(content, mission);
    };
    replace_mission(content, body_start, mission)
}

/// Replace the Mission section that starts at `body_start`.
fn replace_mission(content: &str, body_start: usize, mission: &str) -> String {
    splice(
        content,
        body_start,
        mission_section_end(content, body_start),
        mission,
    )
}

/// The file with a Mission section carrying `mission` put on top.
fn with_missing_mission(content: &str, mission: &str) -> String {
    let mut out = String::with_capacity(content.len() + mission.len() + 32);
    out.push_str("# Task Understanding\n\n## Mission\n");
    out.push_str(mission);
    out.push_str("\n\n");
    out.push_str(content.trim_start());
    out
}

/// Replace `content[start..end]` with `mission`, keeping one blank line
/// before the section that follows.
fn splice(content: &str, start: usize, end: usize, mission: &str) -> String {
    let mut out = String::with_capacity(content.len() + mission.len());
    out.push_str(&content[..start]);
    out.push_str(mission);
    if end < content.len() {
        // One blank line, then the next section exactly as it was.
        out.push_str("\n\n");
        out.push_str(&content[end..]);
    } else {
        out.push('\n');
    }
    out
}

/// Where the current `mission` ends inside the section body, when the body
/// starts with it: `Some(mission.len())`. `None` when the body holds a
/// different mission — text glued to the last character of `mission` is part
/// of a longer line, not the end of it.
fn mission_tops_section(body: &str, mission: &str) -> Option<usize> {
    if !body.starts_with(mission) {
        return None;
    }
    (body.len() == mission.len() || body[mission.len()..].starts_with('\n'))
        .then_some(mission.len())
}

/// End of the Mission section measured from `from`: the heading the stock
/// template puts after Mission, else the next heading of level 1–2 or a
/// `---` rule, else the end of the file.
fn mission_section_end(content: &str, from: usize) -> usize {
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
/// boundary or on a newline.
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
            task_understanding_mission("free notes\n## Observations\nx"),
            None
        );
        assert_eq!(task_understanding_mission(""), None);
    }

    #[test]
    fn replaces_the_mission_and_keeps_everything_else() {
        for template in [API_TEMPLATE, EXECUTOR_TEMPLATE] {
            let out = task_understanding_with_mission(template, "  NEW MISSION \n");
            assert_eq!(
                task_understanding_mission(&out),
                Some("NEW MISSION"),
                "{out}"
            );
            let after = out.split("NEW MISSION\n\n").nth(1).expect("rest");
            let original_after = template.split("OLD MISSION\n\n").nth(1).expect("rest");
            assert_eq!(
                after, original_after,
                "sections after Mission must be untouched"
            );
            assert_eq!(
                out.split("## Mission\n").next(),
                template.split("## Mission\n").next(),
                "everything before Mission must be untouched"
            );
        }
    }

    #[test]
    fn mission_as_last_section() {
        let out = task_understanding_with_mission("## Role\nagent\n\n## Mission\nold", "new");
        assert_eq!(out, "## Role\nagent\n\n## Mission\nnew\n");
    }

    #[test]
    fn restores_a_missing_mission_above_the_agents_notes() {
        // The T4 200366 shape: the agent replaced its task with one
        // observation. The task comes back on top; the note is kept.
        let corrupted =
            "# NE301PIR-200366 任务理解\n## 图像数据路径\n- 无图像数据路径，需设备日志搜索\n";
        let out = task_understanding_with_mission(corrupted, "只在工作日告警");
        assert_eq!(
            out,
            format!("# Task Understanding\n\n## Mission\n只在工作日告警\n\n{corrupted}")
        );
        assert_eq!(task_understanding_mission(&out), Some("只在工作日告警"));
    }

    #[test]
    fn empty_file_gets_just_the_mission() {
        // read_agent_custom_file yields "" for a missing file.
        let out = task_understanding_with_mission("", "task");
        assert_eq!(out, "# Task Understanding\n\n## Mission\ntask\n\n");
    }

    #[test]
    fn idempotent_for_an_up_to_date_file() {
        assert_eq!(
            task_understanding_with_mission(API_TEMPLATE, "OLD MISSION"),
            API_TEMPLATE
        );
        assert_eq!(
            task_understanding_with_mission(EXECUTOR_TEMPLATE, "OLD MISSION\n"),
            EXECUTOR_TEMPLATE
        );
    }

    /// The mission is a verbatim copy of the prompt. A prompt with markdown
    /// headings or `---` lines used to be cut at the first of them — the file
    /// then read as out of sync on every run and grew by one prompt each time.
    #[test]
    fn a_prompt_with_markdown_headings_is_not_resynced_forever() {
        let mission =
            "每天 8 点巡检。\n\n## 判定标准\n\n- 有人赌博才告警\n\n---\n\n# 附注\n周日跳过";
        let file = task_understanding_with_mission(API_TEMPLATE, mission);

        assert_eq!(
            task_understanding_with_mission(&file, mission),
            file,
            "in sync: the file must not be rewritten"
        );
        // Nothing of the template is lost, and nothing is duplicated.
        assert_eq!(file.matches("## Bound Resources").count(), 1, "{file}");
        assert!(file.ends_with("## Notes\nlearned notes\n"), "{file}");
        assert_eq!(file.matches(mission).count(), 1, "{file}");
    }

    /// A prompt edit: the old mission keeps the new one as its first lines
    /// and what was removed starts with a heading. The per-run check cannot
    /// tell that heading from the next section of the file; the edit path
    /// can, and takes the old wording with it.
    #[test]
    fn a_stale_mission_with_headings_is_replaced_whole() {
        let old = "每天巡检。\n## 判定标准\n用 vision 工具分析该图片";
        let file = task_understanding_with_mission(API_TEMPLATE, old);

        let out = task_understanding_repoint_mission(&file, "每天巡检。");

        assert_eq!(
            task_understanding_mission(&out),
            Some("每天巡检。"),
            "{out}"
        );
        assert!(!out.contains("vision"), "{out}");
        assert!(!out.contains("## 判定标准"), "{out}");
        assert!(
            out.contains("## Bound Resources\nNone (free mode)"),
            "{out}"
        );
    }

    /// The per-run check stays conservative: a section the agent put under
    /// the mission, with a heading of its own, is not touched when the
    /// mission itself is current.
    #[test]
    fn the_per_run_check_leaves_the_agents_sections_alone() {
        let mission = "每天巡检。";
        let file = API_TEMPLATE.replacen(
            "## Mission\nOLD MISSION\n\n",
            "## Mission\n每天巡检。\n\n## 观察记录\n- 周一设备离线\n\n",
            1,
        );
        assert_eq!(task_understanding_with_mission(&file, mission), file);
    }

    /// A prompt edit also re-points a file that already carries the new
    /// mission — the edit path does not guess.
    #[test]
    fn repointing_an_in_sync_file_is_a_no_op() {
        let file = task_understanding_with_mission(API_TEMPLATE, "巡检。");
        assert_eq!(task_understanding_repoint_mission(&file, "巡检。"), file);
        assert_eq!(
            task_understanding_repoint_mission(EXECUTOR_TEMPLATE, "OLD MISSION"),
            EXECUTOR_TEMPLATE
        );
    }

    /// Shortening the prompt leaves none of the old wording under the new
    /// mission — the T4 failure, where "用 vision 工具" outlived its removal.
    #[test]
    fn a_shortened_prompt_takes_its_old_lines_with_it() {
        let file =
            task_understanding_with_mission(API_TEMPLATE, "巡检东坪山。\n用 vision 工具分析该图片");

        let out = task_understanding_with_mission(&file, "巡检东坪山。");

        assert_eq!(
            task_understanding_mission(&out),
            Some("巡检东坪山。"),
            "{out}"
        );
        assert!(!out.contains("vision"), "{out}");
        // Stable from here on.
        assert_eq!(task_understanding_with_mission(&out, "巡检东坪山。"), out);
    }

    /// A longer line is a different mission, not the current one plus a note:
    /// the old tail goes.
    #[test]
    fn a_longer_old_line_is_not_mistaken_for_the_new_mission() {
        let file = task_understanding_with_mission(API_TEMPLATE, "巡检东坪山，顺带看水位");
        let out = task_understanding_with_mission(&file, "巡检东坪山");
        assert_eq!(
            task_understanding_mission(&out),
            Some("巡检东坪山"),
            "{out}"
        );
        assert!(!out.contains("水位"), "{out}");
    }
}
