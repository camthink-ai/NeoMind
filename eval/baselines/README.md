# Eval run data — NOT stored in this repo

Score files, per-run logs, and archives are intentionally untracked here.
This repo keeps only the harness (`eval/`), the case library (`eval/cases/`),
and the analysis documents (`docs/model-*.md`).

Canonical locations for run data:

1. **HF private repo** (offsite, authoritative):
   `CamThink-AI/neomind-agent-eval` → `batches/<batch-name>/`
   - 2026-10-29 official-params batch: `batches/2026-10-29-official-params/`
     (20 runs' scores.jsonl + served_model.json + process logs, tar.gz)
2. **Local complete archive** (evaluation machine):
   `~/Desktop/neomind-eval-20261029-complete.zip` (Mac + T4 raw runs, logs, docs)
3. **Raw run dirs** (working copies, gitignored):
   Mac `eval/runs/` · T4 `~/llm-eval/eval/runs/`

Score semantics: pass = `cmd_ok AND (sq_all_pass is not False)`; every published
number is recomputable from the raw `scores.jsonl` (see the audit section of
`docs/model-eval-full-report-20261030.md`).

If you need a baseline for `run_eval.py regression --baseline`, download the
batch from HF and pass the local path, e.g.:

    AGENT_LLM_* ... python3 eval/run_eval.py regression --rounds 2 \
        --baseline ~/downloads/2026-10-29-official-params/mini-spark-x26-17b-think/scores.jsonl
