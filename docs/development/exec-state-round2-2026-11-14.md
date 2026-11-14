# Execution-state Round 2 (2026-11-14)

Spec: store `internal/exec-state-round2.md`. Tip chain on local `1.3.0` from Round 1 `a46770f1`. **No push.** 138 untouched.

## Added
- Real **Operation records** (`OperationRecord`): `operation_id` (UUID), `origin_turn_id`, `tool_name`, `target`, `arguments`, `status`, `result` (`StructuredResult`), `evidence`.
- Status lifecycle: `prepared → running → succeeded | failed | unknown` (+ `cancelled` for Correct of not-yet-executed ops). Device delivery stays separate evidence.
- Same-op resume → same `operation_id`, `prefer_known_result` / `WriteDispatch::PreferKnown` (no re-exec).
- Timeout / unknown → `OperationStatus::Unknown`; inquire; **no** auto-replay; **no** pretend success; **no** write-ledger fact.
- User-authorized re-run → `begin_new_operation(...)` new UUID (no phrase whitelist). Tool/model path wires via `evaluate_mutating_gate` → `begin_new_operation` on first allowed attempt.
- Correct → `cancel_prepared_operations()`; executed facts kept.

## Production wiring
- `stream_core` + `stream_multimodal`: mutating writes go through `evaluate_mutating_gate`; results through `finalize_operation_from_tool_result`.
- `ToolExecutionResult.operation_id` carries the tracked Operation across exec → finalize.

## Acceptance evidence
```bash
export PATH="$HOME/.rustup/toolchains/1.92.0-aarch64-apple-darwin/bin:$PATH"
cargo test --locked -p neomind-agent --lib task_continuity
# 23/23 including Round 2:
# - timeout_unknown_blocks_auto_replay_and_is_not_success
# - same_op_resume_prefers_known_result
# - correct_cancels_prepared_keeps_executed_facts
# - cross_business_write_fingerprints_share_operation_scope
# - weather_to_screen_regression_continue_blocks_reexecute_allows
```

## Round 3 readiness (deferred)
- `details_ref` context packing for Operation/result bodies.
- Tool capability flags (replay-safe vs unknown-on-interrupt) surfaced on Operation records.
- Progress-based loop detection beyond G3 identical-round halt (minimal G3 hooks already present).
