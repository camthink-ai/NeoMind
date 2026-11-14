# §12 P1 — 新用户轮清除临时 Skill + 多轮衔接（2026-11-14）

## Tip / 约束
- Start: `1161e482` on `1.3.0`; no side branch / push.
- Eval 138 (PID 89912, `:29431`) **untouched**.

## Contract
| Layer | New user turn | Survives across turns |
|-------|---------------|------------------------|
| Transient skill bodies (`LlmInterface`) | **Cleared** via `begin_user_turn_clear_transient_skills` | No — reload via skill tool within the turn |
| `TaskState` (goal / steps / write fingerprints) | Updated only by Continue / Correct / Cancel on `push_message` | **Yes** |

Order (text): `push_message` → stream entry skill clear.  
Order (multimodal, fixed this slice): skill clear → `push_message` → first LLM call (directives applied before write gate / tools).

## Coverage tests (deterministic)
- Continue after successful write: skills empty, `may_execute_write` still false.
- Correct: skills empty, goal/object updated, next_action cleared.
- Cancel: skills empty, writes blocked, Continue ignored.
- Post-write recovery: reload skill in new turn OK; completed write still gated.

```bash
cargo test --locked -p neomind-agent --lib task_continuity
# 14/14
```

## Not claimed
- Live Spark / T4 pairing.
- Permanent retention of skill catalogs across turns (explicitly refused).

## Next
1. After 138 frees `:29431`: G2/G3 + skill-clear live probes.
2. D / E / F; human C UX still parked.
