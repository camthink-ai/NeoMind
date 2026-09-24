//! The values a CLI flag accepts, everywhere a model reads them.
//!
//! The CLI does not validate these strings — it forwards them, and the API
//! parses them against a Rust enum. So a value the help omits is one the model
//! will never try, and a value the help invents is one that gets rejected at
//! the far end after a round trip. Both have happened: `notify.on` lost
//! `judgment` (its own test now covers that), and `--schedule-type` still says
//! `interval | cron | event` while the API accepts `manual`.

use neomind_data_push::types::PushTargetType;
use neomind_storage::{ExecutionMode, MemoryMode, ScheduleType};

const COMMANDS: &str = "crates/neomind-cli-ops/src/dispatch/commands.rs";

fn commands_source() -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join(COMMANDS))
        .unwrap_or_else(|e| panic!("reading {COMMANDS}: {e}"))
}

/// The name each value goes by on the wire — read off the enum itself, so this
/// cannot drift from what the API actually parses.
fn wire<T: serde::Serialize>(variants: &[T]) -> Vec<String> {
    variants
        .iter()
        .map(|v| {
            serde_json::to_value(v)
                .expect("serialises")
                .as_str()
                .expect("a wire name is a string")
                .to_string()
        })
        .collect()
}

/// The `///` block that starts at the first line containing `marker`.
///
/// The docs are wrapped, so an arg's values can be spread over three lines;
/// checking the first line alone would call a complete list incomplete.
fn doc_block(source: &str, marker: &str) -> Option<String> {
    let lines: Vec<&str> = source.lines().collect();
    let start = lines.iter().position(|l| l.contains(marker))?;
    let mut block = Vec::new();
    for line in &lines[start..] {
        let trimmed = line.trim();
        if !trimmed.starts_with("///") {
            break;
        }
        block.push(trimmed.trim_start_matches("///").trim());
    }
    Some(block.join(" "))
}

fn assert_documented(marker: &str, values: &[String]) {
    let source = commands_source();
    let block = doc_block(&source, marker)
        .unwrap_or_else(|| panic!("`{marker}` is no longer in {COMMANDS} — update this test"));
    for value in values {
        assert!(
            block.contains(value.as_str()),
            "`{marker}` does not name `{value}`, so a model reading the help will never \
             use that value.\n  help says: {block}"
        );
    }
}

#[test]
fn every_schedule_type_the_api_accepts_is_offered_by_the_cli_help() {
    // `manual` is the one that went missing — and it is the shape the editor's
    // "on demand" strategy compiles to, so the CLI could not create one.
    let values = wire(&[
        ScheduleType::Interval,
        ScheduleType::Cron,
        ScheduleType::Event,
        ScheduleType::Manual,
    ]);
    assert_documented("Schedule type:", &values);
    assert_documented("New schedule type:", &values);
}

#[test]
fn every_execution_mode_the_api_accepts_is_offered_by_the_cli_help() {
    let values = wire(&[ExecutionMode::Focused, ExecutionMode::Free, ExecutionMode::Structured]);
    assert_documented("Execution mode:", &values);
    assert_documented("New execution mode:", &values);
}

#[test]
fn every_memory_mode_the_api_accepts_is_offered_by_the_cli_help() {
    let values = wire(&[MemoryMode::Tool, MemoryMode::Assistant]);
    assert_documented("New memory axis:", &values);
}

#[test]
fn every_push_target_type_the_api_accepts_is_offered_by_the_cli_help() {
    let values = wire(&[PushTargetType::Webhook, PushTargetType::Mqtt]);
    assert_documented("Target type (webhook", &values);
}
