# LFM2.5-2.6B — official sampling params (NEGATIVE CONTROL)

2026-08-17 full en run (154 cases) with the official model-card params
(temp 0.1 / top-k 50 / repeat-penalty 1.1) instead of NeoMind's tested
defaults (temp 0.6 / top_p 0.85).

**Verdict: NOT adopted.** cmd_ok 63.9% vs 65.0% default; tool_ok 82% vs
93%; 19/154 (12.3%) cases wedged past the 600s cap vs 0 — low temperature
turns multi-step failures into deterministic tool loops (no sampling
noise to escape them). Command QUALITY on completed cases was better
(72.8% ex-timeout vs 65.0%), but the wedge rate is disqualifying for
unattended edge operation.

Kept as a baseline so the regression gate can push back if anyone retries
low-temperature sampling: compare any new run against this file and the
`lfm26b-full-final` run (defaults) before changing AGENT_LLM_TEMPERATURE.
