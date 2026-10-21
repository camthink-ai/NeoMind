# Edge Model Deployment Guide — MiniCPM5-2B + Vision Dual-Model Recipe

NeoMind runs best on a single edge box with **two small models instead of one**:
a text model that *acts* (drives the agent's tool calling) and a vision model
that *sees* (describes images for the `vision` tool). The measured agent
recommendation is **MiniCPM5-2B** (2026-09 re-evaluation, see below); the
vision slot stays `LFM2.5-VL-3B`.

> **2026-09 harness correction.** The 2026-08 numbers (`cmd_ok`, 154-case)
> were produced by an eval harness with three defects (no production tool set
> registered on the session, phantom 4096 context window, memory pipeline
> dead). All models were re-measured on the corrected harness — production
> 7-tool surface, real `/props`-probed context, seeded sandbox platform,
> working memory extraction; 5 scenarios × 15 turns. Old and new scores are
> NOT comparable. Rankings changed.

## The split

| Role | Model | Measured (2026-09 corrected harness) | Why |
|---|---|---|---|
| **Agent** (tool calling) | `MiniCPM5-2B` (text) | **64 overall 8K (81% tool accuracy)** — best-in-class CLI domain selection at 2B and flat across context windows | 1.5 GB (Q4_K_M); native OpenAI-format tool calling incl. parallel calls; ~120 tok/s solo decode on M4 Pro (2026-10). Serve at **128K context** (product default since 2026-10, auto-capped to device RAM) — the harness's 8K optimum assumed a light prompt; the production platform prompt (system+tools+memory) weighs 4-6K tokens and starves anything below 16K (see streaming `effective_history_budget`) |
| **Perception** (vision) | `LFM2.5-VL-3B` (vision) | 10% as an agent (2026-08) — **do not use it as the agent** | Strong vision (ScreenSpot 80.7, OCR-class benchmarks), but the vision training materially degraded its tool calling despite sharing the 2.6B backbone |

Both models speak OpenAI-compatible function calling through llama.cpp's
`--jinja` chat-template path, verified end-to-end against NeoMind's agent loop.

### Final 2026-09 context-response matrix (12-scenario suite: zh+en mirrors, 40-turn long-horizon, tools-breadth; seeded sandbox; every UNSTARRED cell the same protocol — starred cells are the older 5-scenario suite at that window)

| Model | 8K | 16K | 32K | Context response profile |
|---|---|---|---|---|
| Qwen3.5-4B | 37.9 | **69.5** | 65.8 | Starved at 8K, peaks at 16K (tool 73.6%, recall 50%, context 82%). Product default 128K (RAM-capped). |
| MiniCPM5-2B | **64.0** (R24) | 61.4 | ~68* | Flat — most robust. Product default 128K (RAM-capped since 2026-10): flat across windows, and production prompt overhead (4-6K tokens) rules out 8K/16K for long agent turns. |
| Ling-3.0-tiny | 61.7 (R24) | 47.8 | 44.9 | Cliff between 8K and 16K — keep sessions short. Product default 128K (RAM-capped). |
| gemma-4-E2B | 59.1* | — | 59.7 | Flat / long-context-stable. |
| LFM2.5-2.6B | 41.3* | — | 53.5 | Improves with context. |

(* = older 5-scenario suite at that window; R24 = two full cycles, n=222 tool
judgments. Cross-window comparisons for a model are decision-grade; ≤6-point
cross-model gaps are statistical ties.)

## Fairness view (2026-09-10): classic scoring exaggerated the differences

The classic score counts ONLY "emitted the right domain command" — it awards
zero for investigation probes (`--help`) and for correct answers given
without a tool call, biasing against careful models. Re-judging every domain
turn (full command 1.0 / exploration 0.5 / substantive direct answer 0.75):

| Model | Classic domain acc | Fair domain score | Bias delta |
|---|---|---|---|
| Qwen3.5-4B 16K | 61.0% | 87.8% | **+26.8pp** |
| Ling 8K | 77.9% | 89.6% | +11.7pp |
| MiniCPM5-2B 8K | 73.4% | 83.1% | +9.7pp |

Under fair judging the three contenders CONVERGE to 83–90% — statistically one
tier of tool competence. The classic leaderboard's cross-model gaps were
largely scoring bias, not capability. Selection therefore rests on the real
differentiators: footprint, latency, context-regime fit, and the outcome
dimensions (resource creation, memory) — which is exactly what the
context-response matrix above encodes. (Classic scores remain the primary
report; the fairness view ships in the harness as a standard second block.)

Bilingual: zh≈en for every contender (MiniCPM5 60/64, Ling 61/58, Qwen16K 55/64)
— English parity is not a differentiator. Long-horizon (40-turn)
memory: 0% for ALL models at ALL windows — the ceiling is the platform-side
extraction/window budget, not model choice. Non-shell tool breadth (file/web/skill/
memory selection): 33–67% everywhere — a shared weak spot.

Selection: **MiniCPM5-2B 32K** = default (1.5 GB, 3 GB floor, Apache-2.0, robust);
**Qwen3.5-4B 16K** = strongest agent when 4 GB+ RAM and 16K ctx fit; **Ling 8K** =
fast short-burst MoE (6 GB+); **gemma 32K** = long-session stability.

Memory-recall caveat: the score depends on both the context window and the
model's own fact-extraction quality (the extractor is the model under test).
At 8K, planted facts are the first casualty of history trimming. A platform-side dedicated extractor
is the highest-leverage fix.

## 2026-10 regression-gate re-verification

All models re-ran against the **1.2.2** platform build on a shared llama-server
(request sampler temp 0.6 / top_p 0.85, thinking on, 33-case hard-signal
regression gate, one round per case — single-round noise floor ±7pp). The gate
has since grown to 40 cases, and 1.2.3 changed the chat guard policy
(labels-only), so the numbers below are 1.2.2 measurements; a 1.2.3 re-run is
pending:

| Model | Pass | Notes |
|---|---|---|
| Nanbeige4.2-3B Q4_K_M (official agent sampler: temp 1.0) | 23/28 (82%) | catalog candidate — see below |
| LFM2.5-24B-A2B Q4_0 | 24/30 (80%) | not in the catalog; 3 timeouts |
| MiniCPM5-2B (official sampler point) | 23/30 (77%) | validates catalog v6's official-first sampler decision |
| LFM2.5-2.6B QAD | 23/32 (72%) | |
| Gemma4-E2B QAT | 22/32 (69%) | |
| Nanbeige4.2-3B Q4_K_M (platform-unified 0.6/0.85) | 12/33 (36%) | same model as the 82% row — sampler point, not capability |

Sampler point is the dominant variable for thinking small models: MiniCPM5-2B
moved 39→77 and Nanbeige 36→82 when switched from ad-hoc/unified points to the
model card's official one. Always evaluate a catalog candidate at its official
card values before ranking it. Nanbeige caveats (2026-10, M4 Pro): Looped
Transformer → slowest decode of the field (~49 t/s solo); temp 1.0 lengthens
thinking and 5/33 cases hit the 600s wall-clock cap (excluded from the pass
denominator); its official card recommends XML tool-call format while NeoMind's
custom-endpoint path teaches the JSON text protocol — native-tools adaptation
is the likely upside path. Context was verified NOT a factor (peak sequence
20.6K/32K, zero truncations).

The non-Nanbeige rows ran on a default 4-slot server. The platform sends
background tasks (memory extraction, system context) concurrently with the
foreground chat, and 4 slots split GPU throughput 3-4× (2026-10: 17 overlapping
request pairs measured; per-request decode 2.4-12 t/s vs 49 t/s solo). Their
pass rates are therefore floors. Both Nanbeige rows ran with **`--parallel 1`**
(no contention, 0 truncations, peak sequence 20.6K/32K). Serve eval traffic
with `--parallel 1` so the foreground stream gets full throughput, and warm the
model with a request or two before the first case (cold-start costs ~5× on the
first generation).

## Serving (llama.cpp)

```bash
# Agent — MiniCPM5-2B (text)
# Port 29375 is what the platform's builtin server uses (moved off 8081,
# which collides with llama.cpp's own 8080-era tooling); a custom backend
# registered at another port is fine — just keep `endpoint` in sync.
# Context: the product default is now 128K, auto-capped to device RAM; for
# manual serving start at 32768 and raise toward 131072 as memory allows.
llama-server -m MiniCPM5-2B-Q4_K_M.gguf \
  --host 127.0.0.1 --port 29375 -ngl 99 -c 32768 \
  --jinja --alias MiniCPM5-2B \
  --temp 1.0 --top-p 0.95 --top-k 40 --min-p 0 --repeat-penalty 1.05

# Perception — LFM2.5-VL-3B (vision; needs its mmproj)
llama-server -m LFM2.5-VL-3B-Q4_K_M.gguf --mmproj mmproj-LFM2.5-VL-3B-F16.gguf \
  --host 127.0.0.1 --port 8082 -ngl 99 -c 131072 \
  --jinja --repeat-penalty 1.0 --top-k 50
```

Non-negotiable settings (MiniCPM5):

- **`--jinja`** — MiniCPM5's function calling is template-rendered XML; without
  the Jinja handler the calls never round-trip into OpenAI `tool_calls`.
- **Context: 128K default (RAM-capped)** — platform default since 2026-10
  (native 131072; was 32K, 2026-09-12). The harness A/B showed a flat overall
  score across 8K/32K (66.2 vs 67.9) with 8K ahead on tool accuracy and
  parallel calls, BUT that measured a LIGHT prompt: the real platform prompt
  (system + tool definitions + memory/skill context) weighs 4-6K tokens, which
  starves an 8K window and triggered constructed overflows (see
  `stream_core::effective_history_budget`). 32K+ restores usable history
  headroom at no measured accuracy cost; the spawn auto-caps to available
  device RAM. Long agent loops on slow models legitimately grow past 20K
  tokens — truncation breaks multi-round tool flows, so size for the workload.
- **`--temp 1.0 --top-p 0.95 --top-k 40`** — model-card recommendation, used
  server-side as default; NeoMind's requests carry their own sampler (temp 0.6)
  which overrides it — both were validated working.
- **`--min-p 0 --repeat-penalty 1.05`** — the official card's anti-repetition
  pair (OpenBMB #374: llama.cpp's 0.05 min_p default causes runaway loops).
  Validated 2026-10 on the 33-case regression gate (1.2.2 build): **77% — the
  model's best measured sampler point**. Do not crank the penalty higher: a
  1.15 recipe degraded structured tool-call output (39%, 2026-10).

Quantization: Q4_K_M is the sweet spot measured here. Q6_K was not observed to
matter (test aborted — effect below noise for the effort).

## Registering in NeoMind

1. **Agent backend (active)**: Settings → LLM Backends → add a
   **llama.cpp** backend pointing at the *text* server
   (`http://<host>:8081` for YOUR OWN manual llama.cpp, or `29375` for the
   platform's builtin server; no `/v1` suffix — the llamacpp client appends its
   own path), then activate it. This is the model that drives chat and
   scheduled agents. (An OpenAI-compatible registration also works, but its
   endpoint must carry `/v1` — that protocol does not auto-append it.)
2. **Perception backend (non-active)**: add a second backend for the *vision*
   server and leave it **not active**. NeoMind's built-in `vision` tool
   automatically prefers dedicated multimodal backends over the active one
   (see `crates/neomind-agent/src/toolkit/vision.rs` — candidate order:
   `model` pin → explicit `vlm_backend_id` → other multimodal instances →
   active backend last), with health-based demotion for backends that fail or
   fake vision. No code or config beyond registering is needed.

Result: the agent plans and executes CLI commands with the fast text model,
and transparently delegates "look at this image" to the VL model — including
images arriving via `/api/images/...` from cameras.

## Licensing note

**MiniCPM5-2B is Apache-2.0** — bundlable in the Docker image/installer
without restriction, and now the catalog's recommended agent
([Abiray/MiniCPM5-2B-GGUF](https://huggingface.co/Abiray/MiniCPM5-2B-GGUF)).

LFM2.5 models are **`lfm1.0` (Liquid AI proprietary)** — NeoMind cannot bundle
them in the Docker image or installer. Users download the GGUFs themselves
([LFM2.5-2.6B-GGUF](https://huggingface.co/LiquidAI/LFM2.5-2.6B-GGUF),
[LFM2.5-VL-3B-GGUF](https://huggingface.co/LiquidAI/LFM2.5-VL-3B-GGUF)) and
serve them locally (the VL model remains the vision-slot recommendation).

## Troubleshooting

| Symptom | Cause / fix |
|---|---|
| Every tool call fails, model produces plain text | Missing `--jinja` on the server |
| 404 on every request from NeoMind | Endpoint/type mismatch: **llama.cpp** backends take no `/v1` (client appends it); **OpenAI-compatible** backends need `/v1` in the endpoint; Anthropic accepts either |
| Agent picks `skill`/wrong tool constantly on a ≤3B model | You are on an old NeoMind build with the bloated `shell` description — upgrade (fixed 2026-08; small models avoid huge tool descriptions) |
| Long multi-step deploys die mid-run | Chat turn bound was raised to 2400s; if you run a custom harness, make sure *its* per-turn and per-case budgets exceed the model's realistic completion time (~20+ min for 20-round deploys at edge speeds) |
| Vision works in isolation but agent never "sees" images | The active (text) backend not being multimodal is fine for tool-routed vision, but *user-uploaded chat images* currently require a multimodal active backend — upload via the vision flow instead |

## Sampling: keep temp 0.6 — official 0.1 measured (2026-08-17/18, LFM2.5)

> LFM-specific reference data, retained for provenance; the 2026-09
> MiniCPM5-2B recommendation uses the same temp 0.6 policy (server default
> temp 1.0 / top_p 0.95 per its model card; NeoMind requests carry temp 0.6
> and override it).

LiquidAI's model card recommends `--temp 0.1 --top-k 50 --repeat-penalty 1.1`.
We ran the full 154-case agent suite both ways on 0.9.17:

| Config | cmd_ok (all) | cmd_ok (ex-timeout) | wedged >600s |
|---|---|---|---|
| NeoMind default (temp 0.6 / top_p 0.85) | 65.0% | 65.0% | 0 |
| Official (temp 0.1 / top-k 50 / rp 1.1) | 63.9% | 72.8% | **19 (12.3%)** |

Low temperature makes completed cases *more* accurate (+7.8pp) but turns
multi-step failures into deterministic loops — no sampling noise to escape
them — and 12% of cases burn their whole budget circling. For unattended
edge agents the wedge rate is disqualifying: **use temp 0.6**. Both runs are
archived under `eval/baselines/` as a negative control.

Related: LFM2.5's thinking is integral — do NOT try to disable it. The
template ignores `--reasoning off` and `enable_thinking:false`; the
mechanically-working `--reasoning-budget 0` costs -33pp cmd_ok AND is slower
(failure loops eat the generation savings; 2026-08-18 A/B). Thinking tokens
also bypass `max_tokens`, which is why NeoMind caps delegated max_tokens and
injects loop-steering hints instead.

## Jetson Orin-class measured baseline (2026-08-24/26, two P3767 eng-ref boards)

All numbers on our runtime (llama.cpp b10545, CUDA sm_87, -ngl 99; unified
memory ~46 GB/s effective decode bandwidth — top of the community range).

| Model | gen tok/s | prompt tok/s | 32K ctx @ 8G board | Notes |
|---|---|---|---|---|
| LFM2.5-2.6B QAD (1.5G) | 36.5 | 1262 | ✅ (1.7G free) | 128K native; thinking integral; Mamba (no speculative) |
| Gemma4-E2B QAT (3.1G) | 34.8 | 1197 | ✅ (2.7G free) | SWA KV is cheap; vision via mmproj |
| **Ling-3.0-tiny Q4_K_M (4.8G)** | **~40** | ~58-69* | ❌ (needs ≥16G) | **Fastest agent-tier on this hw** — MoE 7.9B/A1.3B activated; 77% on the 30-case suite (ties Qwen 3.5 4B); bracket tool-call format is baked in by training — works via native tools (server-side parsing), not via text-format prompting |
| Qwen3.5-4B Q4_K_M (2.6G) | 17.8 | 538 | ✅ (1.7G free) | strongest agent score (76%); thinking switchable |

*small-sample prompt figures; generation numbers are stable 128-token runs.

Selection guide on Orin-class (agent scores from the 2026-09 corrected
harness; throughput from the 2026-08 Jetson runs): speed/vision → Gemma;
**top agent overall → Ling-3.0-tiny (16G+ only)**; **best agent per byte →
MiniCPM5-2B** (1.5 GB, 81% tool accuracy, the recommended default on any
RAM); LFM2.5-2.6B remains the long-context (128K) niche pick.

8G boards: Qwen 64K fits only when clean (6.8G free after cleanup); Ling's
4.8G weights + KV need ≥16G (the picker's 6 GB floor steers small boards
away correctly).

Bench gotcha: resumable-download scripts must verify HTTP 2xx before
appending — an HF error page (1018 B) appended mid-file fails the final
SHA check in a way truncation can't fix (wasted a full 4.8G re-download;
transferring a verified copy over LAN was 30× faster than re-fetching).
