# Plan E — detection-chain 10-run preregister (2026-11-14)

Branch `1.3.0`. Product version stays `1.2.11`. No push.

## What “done” means for this slice

Plan §6 item is **预注册验收**: freeze the scheme, inputs, sampling, pass rule, and offline contracts **before** claiming ≥9/10.

| Gate | Status this slice |
|------|-------------------|
| Preregister 5×zh + 5×en matrix | Done — `eval/acceptance/iter-e-detection-chain.json` |
| Harden chat case contract | Done — transform≥1, rule≥2, preserve `rule-battery-low` |
| Offline validate / precheck / acceptance unit test | Done |
| Deterministic platform chain (`scenario_a_…`) | Done (API-level; not Chat score) |
| Live Spark 10 trials ≥9/10 | **Deferred** — PID 89912 still owns llama `:29431` |

## Scheme

Camera `green-01` detections → person-count Transform → Rule (count > 2) → notify. Ambient `rule-battery-low` must remain (forbidden side effect).

## Contending with 138

- Live job: PID **89912**, `eval-138`, llama **127.0.0.1:29431**, worktree `8ef0@830b54c1` (≠ tip).
- This slice: **touched: false**. Do not point Plan E LLM runs at 29431.
- After 138 exits: launch tip binary + separate llama (e.g. **29432**), 10 trial run-dirs under `eval/runs/1.3.0-iter-e-*`, score ≥9/10 per acceptance file.

## Deferred coverage (needs multi-turn + runtime probes)

Test-before-enable, threshold/notify retarget, fault diagnosis, cancel, interrupt-recover, behavior probes (normal / over-limit / recovery / cooldown). Tracked under F / E2E01 expansion.

## Next

**F：48 场景分组冻结、基线候选配对、最终 138 与 Appliance 验收**（含 Plan E live 9/10 窗口若 138 已结束）
