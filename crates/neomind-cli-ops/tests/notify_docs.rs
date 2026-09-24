//! The notify triggers, everywhere a model reads them.
//!
//! `NotifyOn` gained `judgment` and four places kept advertising the old
//! two-value form: the `--notify` help on `agent create` and on `agent update`,
//! the error message the model sees when its JSON is rejected, and the CLI
//! synopsis in the `agent-management` skill. Nothing failed — the model simply
//! never learned the variant existed, so the feature was unreachable through
//! chat no matter how well the backend supported it.
//!
//! These are the assertions that would have caught it.

/// Every `NotifyOn` variant, by the name that goes on the wire.
///
/// Spelling them out is the mechanism: a fourth variant cannot be added
/// without editing this line, and the moment it is, the checks below fail until
/// the help text, the error message and the skill have caught up.
const TRIGGERS: [&str; 3] = ["failure", "always", "judgment"];

/// Files the model reads before it writes a `--notify`, relative to the
/// workspace root.
const PLACES: [&str; 3] = [
    "crates/neomind-cli-ops/src/agent_cmd.rs",
    "crates/neomind-cli-ops/src/dispatch/commands.rs",
    "crates/neomind-agent/src/skills/builtins/agent-management.md",
];

fn workspace_file(relative: &str) -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join(relative))
        .unwrap_or_else(|e| panic!("reading {relative}: {e}"))
}

#[test]
fn every_trigger_is_named_in_every_place_the_model_reads() {
    for place in PLACES {
        let text = workspace_file(place);
        for trigger in TRIGGERS {
            assert!(
                text.contains(trigger),
                "{place} never names the `{trigger}` trigger, so a model reading it \
                 cannot use that variant"
            );
        }
    }
}

#[test]
fn no_place_lists_the_triggers_in_the_old_two_value_form() {
    // The specific regression: a line enumerating the values that stops at
    // `always`. Matching the pipe-joined form rather than any mention of the
    // words keeps this from firing on prose that happens to discuss one of them.
    for place in PLACES {
        for (n, line) in workspace_file(place).lines().enumerate() {
            if line.contains("failure|always") {
                assert!(
                    line.contains("judgment"),
                    "{place}:{} enumerates the notify triggers without `judgment`: {}",
                    n + 1,
                    line.trim()
                );
            }
        }
    }
}
