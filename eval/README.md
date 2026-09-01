# NeoMind Chat Agent Eval System

A Claude-as-judge eval harness for the NeoMind chat AI agent. Python runner
spawns a real `neomind serve` subprocess per case (full isolation), drives the
agent through the **production chat pipeline** (`POST /api/sessions/:id/chat`),
captures tool-call traces, and a Claude (Anthropic API) session grades the
output across 6 dimensions.

## Layout

```
eval/
├── README.md                   # this file
├── requirements.txt            # requests, anthropic
├── run_eval.py                 # CLI entrypoint (validate / run-case / smoke / run / report)
├── lib/                        # runner + judge library
│   ├── server.py               # spawn neomind serve, configure LLM, HTTP chat
│   ├── seed.py                 # seed fixtures via HTTP
│   ├── state_query.py          # 12 state-query types (spec §4a)
│   ├── fallback.py             # suspected_fallback heuristic (spec §7)
│   ├── validate.py             # case-schema checks (spec §13 step 7)
│   ├── judge.py                # Claude-as-judge via anthropic SDK
│   └── report.py               # aggregate scores.jsonl → grade-card.md
├── fixtures/                   # seed-default.json, seed-empty.json
├── smoke/                      # 5 sanity cases (good-001..003, bad-001..002)
├── cases/
│   ├── zh/<workflow>/*.json    # Chinese Tier 1 cases
│   └── en/<workflow>/*.json    # English Tier 1 cases (shared IDs)
├── runs/                       # auto-created per run (gitignored)
└── reports/                    # auto-created (gitignored)
```

## Architecture (pivoted 2026-06-29)

The agent under test runs INSIDE the `neomind serve` subprocess via the
production chat pipeline. This exercises the real system prompts, real tool
registry, real multi-round tool-calling continuation, real list-only-dead-end
detection — everything the chat UI exercises. The previous in-process
`SessionManager` (Rust `eval-runner` crate) bypassed all of that and caused
multi-tool calls to silently fail in eval despite working fine in the chat UI.
The Python runner uses HTTP only and has no Rust dependency beyond the
`neomind` binary itself.

Per case:
1. Spawn `neomind serve --host 127.0.0.1 --port <free>` with temp
   `NEOMIND_DATA_DIR` and CWD = tempdir.
2. Pre-seed a known API key via `neomind api-key create` BEFORE spawning —
   the subprocess's `AuthState::new()` (CWD-relative path → resolves to the
   same redb) loads that key. Avoids needing to read redb from Python.
3. Pass `NEOMIND_API_BASE` + `NEOMIND_API_KEY` as env on the subprocess so
   the agent's in-process shell dispatch (CLAUDE.md "CLI In-Process Dispatch")
   talks back to the same server.
4. Configure the agent-under-test's LLM backend via
   `POST /api/llm-backends` + `/activate` — same API surface the chat UI uses.
5. Seed fixture + case extras via HTTP POST.
6. Create chat session (`POST /api/sessions`) + run each turn
   (`POST /api/sessions/:id/chat`, 130s timeout).
7. Run state queries via HTTP GET.
8. Run `detect_suspected_fallback` (3 conditions: all turns empty+fast AND
   expectations mention a tool word).
9. Send CaseRecord to Claude (Anthropic API) for grading.
10. Teardown: kill subprocess, drop tempdir.

## Quick Start

```bash
# 0. Install Python deps (one-time)
pip install -r eval/requirements.txt

# 1. Build the neomind binary (release; the runner uses target/release/neomind)
cargo build -p neomind-cli --release

# 2. Validate every case (no server, no LLM)
python3 eval/run_eval.py validate-all --root eval/cases

# 3. Run a single case (requires AGENT_LLM_*)
AGENT_LLM_API_KEY=sk-xxx \
AGENT_LLM_ENDPOINT=https://api.deepseek.com/v1 \
AGENT_LLM_MODEL=deepseek-chat \
AGENT_LLM_BACKEND_TYPE=openai \
AGENT_LLM_ENDPOINT=https://api.deepseek.com/v1 \
  python3 eval/run_eval.py run-case --case eval/smoke/good-002.json

# 4. Run smoke cases (no judge)
AGENT_LLM_API_KEY=... AGENT_LLM_ENDPOINT=... AGENT_LLM_MODEL=... \
  python3 eval/run_eval.py smoke

# 5. Run all Tier 1 cases WITH Claude judge (requires ANTHROPIC_API_KEY)
AGENT_LLM_API_KEY=... AGENT_LLM_ENDPOINT=... AGENT_LLM_MODEL=... \
  python3 eval/run_eval.py run --root eval/cases --lang both --judge

# 6. (Or just generate the grade card from existing scores)
python3 eval/run_eval.py report --scores eval/runs/<ts>/scores.jsonl
```

## Case Schema (spec §3)

```json
{
  "id": "device-create-temp-sensor",      // language-agnostic, NO zh-/en- prefix
  "lang": "zh",                            // "zh" | "en"
  "category": "device",                    // device|rule|agent|dashboard|transform|message
  "workflow": "device-onboarding",         // sub-folder name
  "scenario_type": "single_turn",          // "single_turn" | "multi_turn"
  "description": "...",                    // human-readable summary
  "setup": {
    "fixture": "seed-default",             // eval/fixtures/<name>.json
    "extras": {                            // case-specific add-ons
      "devices": [], "metrics": [], "rules": [], "agents": [],
      "transforms": [], "dashboards": [], "channels": []
    }
  },
  "turns": [{ "user": "..." }],
  "applies": ["tool_accuracy","task_completion"],   // dimensions to score
  "expectations": {
    "per_turn": ["Turn 1: ..."],           // length MUST equal turns
    "overall": "..."
  },
  "state_queries": [                       // optional — HTTP-verified post-conditions
    { "type": "device_exists", "params": { "id": "x" }, "expected": true }
  ]
}
```

## Dimensions & Weights (spec §5)

| Dimension | Weight |
|-----------|--------|
| tool_accuracy | 25% |
| task_completion | 25% |
| response_quality | 20% |
| context_retention | 15% (multi_turn only) |
| error_recovery | 15% |
| language_adherence | 5% |

Weights renormalize over `applies[]` subset.

## State Query Types (spec §4a)

`device_exists`, `device_count`, `rule_exists`, `rule_enabled`, `agent_exists`,
`agent_status`, `transform_exists`, `dashboard_exists`,
`dashboard_component_count`, `channel_exists`, `message_count`, `push_enabled`.

## Two env-var groups (do not confuse!)

| Var | Purpose | Where |
|-----|---------|-------|
| `AGENT_LLM_API_KEY` / `AGENT_LLM_ENDPOINT` / `AGENT_LLM_MODEL` | Powers the chat agent (model under test) | Set on runner; propagated to server via API |
| `AGENT_LLM_BACKEND_TYPE` | `ollama` / `llamacpp` / `openai` (default) / `anthropic` — vendors ride `openai` + their endpoint | Same as above |
| `AGENT_LLM_THINKING` | `true` / `false` (default; commit c6385169) | Same as above |
| `ANTHROPIC_API_KEY` | Powers the Claude judge | Runner process only |
| `EVAL_JUDGE_MODEL` | Override judge model (default `claude-opus-4-6`) | Runner process only |
| `NEOMIND_TEST_BIN` | Override `neomind` binary path (default `<cwd>/target/release/neomind`) | Runner process only |

The test server auto-derives its OWN API key — never set `NEOMIND_API_KEY`
yourself; the runner pre-seeds one via `neomind api-key create` and propagates
it to the subprocess.

## Known limitations

- **Greeting fast-path false positive**: smoke-good-001 (`你好`) triggers the
  agent's canned greeting response at `agent/mod.rs:1434` (`try_fast_path`) (4ms, no LLM). The
  fallback heuristic flags this as `suspected_fallback=true` because
  expectations mention tool words. This is a known false positive — the agent
  behavior is correct for greetings.
- **Pre-seeded `data/api_keys.redb` quirk**: `AuthState::new()` hardcodes the
  relative path `data/api_keys.redb` (does NOT respect `NEOMIND_DATA_DIR`).
  The runner sets CWD = tempdir so the relative path resolves correctly.

## See Also

- Spec: `docs/superpowers/specs/2026-06-29-eval-system-design.md`
- Plan: `docs/superpowers/plans/2026-06-29-eval-system.md` (historical —
  describes the original Rust crate architecture, now superseded by Python)

## Regression gate (`run_eval.py regression`)

Fast verdict gate over the curated set in `regression_set.txt` (33 cases:
30 domain-stratified + 3 `surface-micro` single-command cases that isolate
CLI-surface accuracy from multi-step planning noise). Resolves cases by JSON
`id` — file location/name is irrelevant, which is what makes the case-library
layout free to reorganize.

```bash
# CI / pre-merge gate (exit 1 on robust PASS->FAIL):
AGENT_LLM_* ... python3 eval/run_eval.py regression --rounds 2

# after merging an approved change, refresh the reference:
AGENT_LLM_* ... python3 eval/run_eval.py regression --rounds 2 --update-baseline
```

- **Use `--rounds 2` for verdicts you act on.** Single-round runs have a
  ~7pp noise floor (observed 2026-09-01: 3/33 flagged regressions on a
  single round, all three PASS on rerun). A single round is fine as a
  quick smoke during development.
- The baseline is per-case robust: multi-round baselines demote
  round-disagreeing cases to `None` (noisy bucket) instead of flip-flopping.
- Cases present in the set but absent from the baseline run without
  affecting the verdict — extend the set first, `--update-baseline` later.
- Note: `--case-id` selection (also used internally by the gate) matches by
  id across BOTH langs and takes the first path (en sorts before zh), so
  the gate exercises the `en` variant of each case.
