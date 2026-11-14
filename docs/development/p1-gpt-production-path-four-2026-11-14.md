# GPT P1 production path — four fixes (2026-11-14)

## Tip / 约束
- Branch `1.3.0` tip chain; **no push**.
- Eval 138 (PID 89912, `:29431`) **untouched**.
- Prior skill-budget commits kept; **these four** are the GPT P1s (do not claim done without them).

## 1. Zero history budget → degrade tools
When `history_select` returns [`HistorySelectReason::ZeroBudget`] with a non-empty prior history, the production assemble path must **not** keep attaching tools (`tools_allowed_after_history_select` → false). Wired in `chat_internal`, `chat_internal_message`, multimodal stream, and `chat_stream_internal_scoped`. Log: `zero_budget_degrade_no_tools`.

**Proof (real request path):**
```bash
cargo test --locked -p neomind-agent --features test-utils --lib zero_budget_history_strips
# llm::request_tests::zero_budget_history_strips_tools_on_real_stream_request
```

## 2. Write fingerprint scope
- `Continue` keeps `completed_write_fingerprints` (resume ≠ replay).
- `SetGoal` / `Correct` clear write scope (new / corrected goal boundary).
- `AuthorizeRerun` (「再发送一次」 / `send again`) clears fingerprints only — authorized re-run of the same goal.

## 3. TaskState on production path
- Ordinary substantive requests → `SetGoal` via `parse_user_directive` (greetings/acks skipped).
- Stream entry sets `LlmInterface::set_task_state_prompt(task_state.to_prompt_section())` (goal / confirmed / next).
- Successful tools call `confirm_step_from_tool` in `stream_core` + multimodal.
- 「继续」 keeps gated writes and injects Active Task State into the model system prompt.

## 4. Shell fingerprint case
`is_mutation_command` lowercases for **detection** only. `mutating_write_fingerprint` fingerprints the **original** command (arg values keep case).

## Gates
```bash
export PATH="$HOME/.rustup/toolchains/1.92.0-aarch64-apple-darwin/bin:$PATH"
cargo test --locked -p neomind-agent --lib zero_budget_must_not
cargo test --locked -p neomind-agent --lib shell_write_fingerprint
cargo test --locked -p neomind-agent --lib task_continuity   # 16/16
cargo test --locked -p neomind-agent --features test-utils --lib zero_budget_history_strips
cargo test --locked -p neomind-agent --features test-utils --lib task_state_prompt_injected
```

## Not claimed
- Live Spark / T4 A/B; human C UX; D/E/F live.
