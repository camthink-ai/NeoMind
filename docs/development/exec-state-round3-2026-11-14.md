# Execution-state Round 3 (2026-11-14)

Spec: store `internal/exec-state-round3.md`. Tip chain on local `1.3.0` from Round 2 `bcc8655c`. **No push.** 138 untouched.

## Added
- **`details_ref`**: large Operation bodies keep full text on the record; model-facing summary is structured needles (`build_structured_summary`) plus optional `details_ref=$cached:op:<id>` — never “first 120 chars” as fact.
- **`ToolCapability`**: `read_only` / `idempotent` / `retry_policy` snapshot on each Operation (shell derived from mutation heuristic).
- **Progress loop**: `progress_loop_decision` — identical no-progress → halt; status poll with changing evidence → timed allow; write unknown → inquire (wired into `stream_core`).
- **Budget pressure order**: skills → old history → mandatory context → stop/compress-retry with **no tools** when feedback cannot fit (`tools_allowed_under_budget_pressure` / ZeroBudget).
- Prompt section lists proven op status + summary + `details_ref` + capability flags.

## Acceptance evidence
```bash
export PATH="$HOME/.rustup/toolchains/1.92.0-aarch64-apple-darwin/bin:$PATH"
cargo test --locked -p neomind-agent --lib task_continuity
# 29/29 including Round 3:
# - structured_summary_is_not_body_prefix_and_sets_details_ref
# - prompt_section_emits_details_ref_and_capability
# - tool_capability_distinguishes_shell_read_vs_write
# - progress_status_poll_changing_evidence_timed_allow
# - budget_pressure_order_and_no_tools_when_feedback_lost
# - weather_to_screen + cross_business regressions
```

## Remaining gaps
- Assemble path does not yet auto-store Operation bodies into `LargeDataCache` under the `$cached:op:` key (ref is declared; file_read resolve for `op:` namespace still thin).
- Skill/history compress-retry is ordered in helpers + ZeroBudget gate; full packer that reshuffles assemble in one place is still incremental.
- Live Spark / tip-138 still blocked — offline unit only.
- Cross-business live probes and Appliance not claimed.
