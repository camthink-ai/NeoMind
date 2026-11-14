# Plan F — 48-scenario freeze + pairing gates (offline slice)

Date: 2026-11-14. Branch `1.3.0`. Version stays `1.2.11`. No push.

## Scope this slice

| Done offline | Deferred (wait for 138 / later) |
|--------------|----------------------------------|
| Freeze 48 ids + family taxonomy | Live tip 138 |
| Stratified splits 24 / 12 / 12 | Appliance 6-task hardware gate |
| Holdout IDs flagged maintainer-only | Plan E Spark 9/10 |
| Provisional maps to existing cases | Full case authoring for stubs |
| Pairing protocol (baseline↔candidate) | Actual paired LLM runs |

## Catalog

Canonical file: `eval/acceptance/business-capability-48.json`

- Families: DEV8 + TR8 + RULE8 + AG6 + AUX6 + E2E12 = 48
- Splits (stratified within family): development 24 / daily_regression 12 / independent_holdout 12
- `case_status`: `stub` | `provisional_map` | `case_ready` (only E2E01 is `case_ready` via Plan E)
- Holdout entries carry `prompt_status: maintainer_only` and **no prompt text**

## Pairing gates (must hold before any paired claim)

1. Same model, runtime, sampling (.3/.75/20, thinking on, 600s), timeout, fixture, grader, hardware.
2. Freeze repetition count **before** launch; interleave arms.
3. Report both-pass / gained / lost / both-fail + absolutes + scenario-cluster intervals + each lost trail.
4. Small n → write **证据不足**; never treat net pp or correlated p as proven uplift.
5. Report business-48 **separately** from historical 138 (`cmd_ok AND sq_all_pass` unchanged).
6. Running PID 89912 / `:29431` is **not** tip baseline (8ef0@830b54c1).

## Contending with 138

PID 89912 untouched. Do not start F live pairs or E 9/10 on `:29431`.

## Offline gates run this slice

```bash
python3 eval/lib/test_business_capability_48.py
python3 eval/lib/test_iter_e_acceptance.py   # E2E01 link still green
```

## Next

**发布前：版本统一、证据归档、未解决问题、人工复现步骤** — or resume F remainder (live 138 / Appliance / E 9/10) once 138 exits.
