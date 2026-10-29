# 2026-10-29 official-params batch baselines

One dir per gate run; each holds `scores.jsonl` (compact hard-signal records,
canonical pass = `cmd_ok AND sq_all_pass is not False`) and `served_model.json`.
Full methodology, tables, and analysis: `docs/model-leaderboard-20261029-official.md`.

Runs: Spark-1.7B off/think · Spark-4B off · LFM3-1.7B off · LFM2.5-2.6B off
(Mac wrong-params + T4 official) / think · Qwen3.5-4B official / 1.0-0.95
counterfactual / think · Gemma-E2B off/think · GLM-5.3 (off) · DeepSeek-flash
(off) · r11 SFT (regression-20261027-171122).

Included: LFM3-1.7B think (47.1%) and MiniCPM5-2B think (45.7%), both from T4
(v0.8.0 CUDA build), dirs rerun-lfm3-17b-think-t4 / minicpm5-2b-think-t4.
Not run: Spark-4B think (cancelled per user; model staged on T4).
