# 2026-10-29 official-params batch baselines

One dir per gate run; each holds `scores.jsonl` (compact hard-signal records,
canonical pass = `cmd_ok AND sq_all_pass is not False`) and `served_model.json`.
Full methodology, tables, and analysis: `docs/model-leaderboard-20261029-official.md`.

Runs: Spark-1.7B off/think · Spark-4B off · LFM3-1.7B off · LFM2.5-2.6B off
(Mac wrong-params + T4 official) / think · Qwen3.5-4B official / 1.0-0.95
counterfactual / think · Gemma-E2B off/think · GLM-5.3 (off) · DeepSeek-flash
(off) · r11 SFT (regression-20261027-171122).

Pending at snapshot time: LFM3-1.7B think, MiniCPM5-2B think, Spark-4B think
(running on T4, dirs rerun-lfm3-17b-think-t4 / minicpm5-2b-think-t4 /
spark-x26-4b-think-t4 — copy in when final).
