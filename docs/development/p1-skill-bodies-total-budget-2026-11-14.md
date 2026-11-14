# §12 P1 — Skill 全文累积总量上限（2026-11-14）

## Tip / 约束
- Start: `16b8176b` on `1.3.0`; no side branch / push.
- Eval 138 (PID 89912, `:29431`) **untouched**; no new long-run.
- Human phone / 5×4 still parked.

## Problem
Distinct skill-tool loads appended full guides into `TransientSkillContext` with
**no total cap**. T4 production (15+ builtin skills) showed short history yet
`estimated_tokens` 18–34k and `available_tokens=0` — fixed prefix (system +
tools + skills) ate the window (`docs` / store T4 inference analysis).

## Fix
- `skill_bodies_token_budget(max_context)` = `(ctx/8).clamp(2_000, 6_000)`
  estimated tokens (32K → 4096).
- `plan_skill_body_append` / `set_skill_context`: FIFO keep earlier loads;
  later loads **truncate** via `truncate_to_tokens` or **skip** when remaining
  &lt; 32 tokens; skipped ids stay marked loaded (no retry-loop re-append).
- Pinned skills in `assemble_system_prompt` use the same total budget after
  per-skill `body_within_budget()`.

## Gates
```bash
cargo test --locked -p neomind-agent --lib skill_bodies_token_budget
cargo test --locked -p neomind-agent --lib plan_skill_body_append
cargo test --locked -p neomind-agent --lib test_set_skill_context
# 4/4
```

## Not claimed
- Live Spark / T4 redeploy A/B.
- That this alone cures production loops (P0 history trim still required on T4).
- stream_core “clear skills each user turn” policy change (separate P1).

## Next
1. Wait for 138 to free `:29431`, then optional T4 skill-load A/B.
2. Remaining plan: stream_core skill-clear / continue; D/E/F; human C UX.
