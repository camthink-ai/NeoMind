# Execution-state Round 1 (2026-11-14)

Spec: store `internal/exec-state-round1.md`. Tip chain on local `1.3.0`. **No push.** 138 untouched.

## Removed
- Utterance-guessed task boundary: no “≥4 chars → SetGoal”.
- 「再发送一次」 / `send again` phrase whitelist as write-gate control (`AuthorizeRerun` directive removed).
- Clearing write fingerprints / confirmed steps to authorize re-run (`clear_write_scope_for_new_boundary`).

## Kept / added
- Full **write ledger** (`WriteFact { operation_id, fingerprint }`) — never wiped for re-auth.
- **Operation-scoped** anti-replay: `may_execute_write` only looks at `current_operation_id`.
- Every user message → `note_user_turn()` (Turn); model interprets supplement / follow-up / correct / new need.
- Explicit control parse only: Continue / Cancel / Correct (Correct does **not** lift anti-replay).
- Explicit re-execute → `begin_new_operation()` (increments op id; ledger + steps retained).
- TaskState prompt injection + `confirm_step_from_tool` production wiring from prior tip kept.
- Zero-budget tool degrade + shell fingerprint case from prior tip kept.

## Acceptance evidence
```bash
export PATH="$HOME/.rustup/toolchains/1.92.0-aarch64-apple-darwin/bin:$PATH"
cargo test --locked -p neomind-agent --lib task_continuity
# 19/19 including:
# - followup_correct_do_not_lift_anti_replay_reexecute_advances_operation
# - weather_to_screen_regression_continue_blocks_reexecute_allows
# - cross_business_write_fingerprints_share_operation_scope
# - ordinary_user_message_is_turn_not_set_goal
```

## Round 2 hooks (deferred)
- Replace numeric `current_operation_id` with real **Operation ID** + structured tool results.
- Wire model/tool path to call `begin_new_operation` (no phrase whitelist).
- Context + progress loops (Round 3).
