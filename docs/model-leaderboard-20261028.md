# NeoMind Agent Model Leaderboard — Bilingual Gate (2026-10-28)

**Methodology**: 69-case regression gate × 2 languages (en+zh) = 138 executions.
Uniform 32K context, each model's official sampling params (temp/top_p),
Q4-class quantization, single round, same host (M-series Mac, Metal GPU),
same `neomind` binary, same day. Hard-signal verdicts only (state queries +
exact command matching); LLM judge not used. `cmd_partial` / `sq_partial`
are informational partial-credit fields (do NOT affect pass/fail).

## Leaderboard

| # | Model | Params (actual) | Strict | cmd_partial | sq_partial | en | zh | Notes |
|---|-------|------------------|--------|-------------|------------|----|----|-------|
| 1 | Qwen3.5-4B (zero-shot) | ~4.4B | **55.1%** | 67.7% | 68.4% | 57% | 54% | Catalog's strongest agent |
| 2 | Spark-X2.6-4B (zero-shot) | ~4.5B | **54.3%** | 58.9% | 65.7% | 58% | 51% | Apache-2.0, 1M ctx |
| 3 | Spark-X2.6-1.7B (zero-shot) | ~2.0B | **47.1%** | 62.1% | 58.2% | 49% | 45% | Best ≤2B class; next fine-tune base |
| 4 | **r11 (MiniCPM5-2B + 1,175 traj SFT)** | 2.5B | **42.8%** | 46.6% | 50.4% | 43% | 42% | Our fine-tune; +9.5pp over base |
| 5 | MiniCPM5-2B (zero-shot) | 2.5B | 33.3% | 69.0% | 41.9% | 33% | 33% | r11's base; high cmd-guess, low completion |
| 6 | LFM2.5-2.6B (zero-shot) | 2.6B | 29.0% | 66.3% | 34.9% | 30% | 28% | Weakest; old 63% single-lang score overestimated |
| — | Gemma4-E2B QAT | 2.6B | (invalid*) | — | — | — | — | 121/138 runtime fails (env contention); rerun needed |
| — | LFM3-1.7B | ~1.7B | (running) | — | — | — | — | Gate in progress (Metal-patched build) |

\* agent_failed > 30% ⇒ run invalid per data-quality rule.

## Key Findings

1. **Spark-X2.6-1.7B is the next fine-tune base.** Zero-shot 47.1% beats our
   fine-tuned r11 (42.8%) with 20% fewer parameters. Same Q4_K_M + 32K ctx
   — a fair comparison. Fine-tuning it with the proven r11 pipeline
   (tokenizer-rendered training + assistant-only loss) targets 55-60%.

2. **What SFT actually taught r11**: NOT command vocabulary (base already
   guesses 69% of commands) but completion — turning "issued a command" into
   "world state changed" (sq 41.9% → 50.4%) and eliminating wrong operations.

3. **Catalog copy corrections needed**: LFM2.5-2.6B (default download) is the
   weakest at 29%; recommend Spark-1.7B or MiniCPM5-2B as the default.

4. **Historical single-language scores overestimated** capability. The old
   en-only gate gave r11 94.2% and LFM2.5 ~63%; bilingual full evaluation is
   the honest yardstick.

## Assets

- Baselines frozen in `eval/baselines/` (in git)
- All runs in `eval/runs/mini-*`, `eval/runs/regression-*`
- r11 model: HF private `CamThink-AI/neomind-agent-internal`
- Eval suite: HF private `CamThink-AI/neomind-agent-eval`

## Infra Notes

- LFM3 required llama.cpp ≥ b11946 (arch support, merged 2026-10-27 PR #31252);
  b11946 Metal shader has a constexpr bug (issue #31272) — patched locally by
  removing `constexpr` from `ggml_metal_unary_cast` in `unary.metal`.
- Gemma rerun pending; Ling-3.0-tiny not scheduled (out of catalog focus).
