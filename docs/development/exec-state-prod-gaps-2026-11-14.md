# Execution-state production gaps (2026-11-14)

Spec: store `internal/exec-state-prod-gaps.md`. Tip chain on local `1.3.0` from Round 3 `b449bd80`. **No push.** 138 untouched.

## Fixed (priority order)
1. **Concurrent lease + new-op entry**: `Prepared` alone takes the atomic Prepared→Running lease; `Running` → `InProgress` (tool runs once). Production `_new_operation: true` calls `begin_new_operation` for same-args re-run; resume without the flag PreferKnown (no replay). Wired in `evaluate_mutating_gate` + stream_core / multimodal.
2. **details_ref storage**: `LargeDataCache::store_operation_body` before publishing `$cached:op:<id>`; session isolation; eviction → clear fail on read; packer keeps intact `details_ref` under budget pressure.
3. **Capability unify + stop halt**: `mutating_write_fingerprint` / `all_tools_were_read_only` / tool_exec retry driven by `derive_tool_capability` (not a parallel name-table). `progress_loop_decision` receives unknown-op; `apply_progress_stop` writes Halted on text + image paths (never leave Active after stop).

## Acceptance evidence
```bash
export PATH="$HOME/.rustup/toolchains/1.92.0-aarch64-apple-darwin/bin:$PATH"
cargo test --locked -p neomind-agent --lib -- task_continuity history_select mutating_write_fingerprint
# Includes four production-path groups:
# - prod_concurrent_same_args_tool_runs_once
# - prod_user_new_op_reruns_resume_old_does_not_replay
# - prod_details_ref_round_trips_expired_clear_fail
# - prod_long_history_polling_stop_paths_consistent
```

## Not claimed
- Live Spark / tip-138 / Appliance
- Full assemble packer beyond details_ref-preserving compress
