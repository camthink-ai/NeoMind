# Eval Baselines

Committed reference runs for **regression detection**. Unlike `eval/runs/`
(gitignored, ephemeral), these are checked in so any contributor can compare
against a known reference.

## Freeze a baseline

Run a full eval with a reference model, then copy the run dir here under a
named model dir:

```bash
AGENT_LLM_API_KEY=... \
AGENT_LLM_ENDPOINT=https://open.bigmodel.cn/api/anthropic/v1 \
AGENT_LLM_MODEL=glm-5.2 AGENT_LLM_BACKEND_TYPE=anthropic \
AGENT_LLM_THINKING=false \
  .venv/bin/python eval/run_eval.py run --root eval/cases --lang both

# copy the latest run into a named baseline
cp -r "eval/runs/$(ls -t eval/runs | head -1)" eval/baselines/glm-5.2
```

Commit `eval/baselines/<model>/` (cases.jsonl, scores.jsonl, grade-card.md).
`eval/baselines/` is intentionally NOT gitignored (only `eval/runs/` and
`eval/reports/` are).

## Compare a new run vs a baseline

```bash
.venv/bin/python eval/run_eval.py compare \
  --baseline eval/baselines/glm-5.2/scores.jsonl \
  --run eval/runs/<timestamp>/scores.jsonl
```

Prints the per-category hard pass-rate delta (positive = improvement). Run
this after any model-facing change (prompt freeze, code-action, SFT round) to
see exactly which categories moved — the whole point of the hard-signal layer.

## Which baselines?

- **glm-5.2** (teacher, GLM-5.2 via the Anthropic-compatible endpoint): the
  "known-good" green reference (~76–81% hard pass on the current suite). The
  canonical baseline.
- Optionally a **pre-SFT MiniCPM5-1B** snapshot as the regression floor for
  the actual target model (it'll change post-SFT, so re-freeze per training
  round, keep the pre-SFT one as the floor).
