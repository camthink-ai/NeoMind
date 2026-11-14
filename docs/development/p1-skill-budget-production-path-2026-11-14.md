# §12 P1 — Skill budget on production paths (2026-11-14)

## Tip / 约束
- Start: `1be81451` on `1.3.0`; no side branch / push.
- Eval 138 (PID 89912, `:29431`) **untouched**.

## Gap (GPT review of prior P1)
Prior P1 capped transient (`set_skill_context`) and pinned (`assemble`) **independently**.
Manifest claimed a shared total, but production assemble could still inject ~2×
budget (large pins + mid-turn skill load). Scheduled-agent `tool_loop`
`skill_reference` also appended full guides with **no** total cap.

## Fix
1. `build_skill_injection_block` — pinned chunks first, then transient
   `## Skill Reference`, under **one** `skill_bodies_token_budget`.
2. `assemble_system_prompt` uses that helper (chat production path).
3. `set_skill_context` reserves pinned estimated tokens so mid-turn loads
   leave room for pins.
4. `ai_agent` `process_tool_results` skill branch uses
   `plan_skill_body_append` + the same budget (scheduled-agent production).

## Gates
```bash
cargo test --locked -p neomind-agent --lib build_skill_injection_block
cargo test --locked -p neomind-agent --lib assemble_system_prompt_pin
cargo test --locked -p neomind-agent --lib executor_skill_reference_respects
cargo test --locked -p neomind-agent --lib test_set_skill_context
# all pass
```

## Not claimed
- Live Spark / T4 A/B; alone curing production loops.
- Changing per-skill `token_budget` metadata or auto-match injection.

## Next
1. After 138 frees `:29431`: optional T4 skill-load A/B with tip.
2. Human C UX; D/E/F live still parked.
