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
| **Agent** (tool calling) | `MiniCPM5-2B` (text) | **81% tool accuracy / 66 overall** — statistically ties cloud deepseek-v4-flash; best-in-class CLI domain selection at 2B | 1.5 GB (Q4_K_M); native OpenAI-format tool calling incl. parallel calls; ~53 tok/s on M4 Pro. Serve at **8K context** — see trade-off below |
| **Perception** (vision) | `LFM2.5-VL-3B` (vision) | 10% as an agent (2026-08) — **do not use it as the agent** | Strong vision (ScreenSpot 80.7, OCR-class benchmarks), but the vision training materially degraded its tool calling despite sharing the 2.6B backbone |

Both models speak OpenAI-compatible function calling through llama.cpp's
`--jinja` chat-template path, verified end-to-end against NeoMind's agent loop.

### Full 2026-09 leaderboard (identical conditions for every model: self-hosted seeded sandbox, 5×15 turns, 8K ctx)

| Model | Tool acc | Memory recall | Resource creation | Overall | Notes |
|---|---|---|---|---|---|
| **Ling-3.0-tiny** | 79.6% | 20% | **77%** | **71.2 (B)** | Top overall — strongest complete workflows. 4.8 GB, needs llama.cpp ≥ b10545 (bailingmoe3) |
| MiniCPM5-2B (32K) | 70.2% | 20% | 54% | 67.9 | Long ctx trades tool accuracy for recall. Not worth it vs 8K |
| **MiniCPM5-2B (8K)** | **81.2%** | 0% | 54% | **66.2** | **Recommended default** — highest tool accuracy, 1.5 GB, Apache-2.0 |
| deepseek-v4-flash (cloud ref) | 57.1% | 50% | 46% | 60.9 | Same tier; investigates before acting, 5× faster per turn |
| gemma-4-E2B | 74.5% | 30% | 0% | 59.1 | Good tool selection, resource creation collapses on real data |
| LFM2.5-2.6B | 66.7% | 0% | 31% | 38.1 | Former default; CLI domain mapping drifts (maps "list devices" to `ls /dev`) |
| Qwen3.5-4B | 61.7% | 10% | 8% | 37.9 | Thinking model investigates deeply (~40 s/turn) but creations fail checks |
| MiniCPM5-1B | 27.3% | 0% | 23% | 26.3 | Below agent threshold |
| Qwen3.5-0.8B | 34.8% | 0% | 15% | 25.9 | Below agent threshold |

n=1 per model (single run each): treat ≤6-point gaps as statistical ties; the
structural gaps (Ling's 77% resource creation, MiniCPM5's 81% tool accuracy,
the 1B-class collapse) are the load-bearing findings.

Memory-recall caveat: the score depends on both the context window and the
model's own fact-extraction quality (the extractor is the model under test).
At 8K, planted facts are the first casualty of history trimming; DeepSeek's
50% is largely free 128K-context reading. A platform-side dedicated extractor
is the highest-leverage fix.

## Serving (llama.cpp)

```bash
# Agent — MiniCPM5-2B (text)
llama-server -m MiniCPM5-2B-Q4_K_M.gguf \
  --host 127.0.0.1 --port 8081 -ngl 99 -c 8192 \
  --jinja --alias MiniCPM5-2B --temp 1.0 --top-p 0.95

# Perception — LFM2.5-VL-3B (vision; needs its mmproj)
llama-server -m LFM2.5-VL-3B-Q4_K_M.gguf --mmproj mmproj-LFM2.5-VL-3B-F16.gguf \
  --host 127.0.0.1 --port 8082 -ngl 99 -c 131072 \
  --jinja --repeat-penalty 1.0 --top-k 50
```

Non-negotiable flags (MiniCPM5):

- **`--jinja`** — MiniCPM5's function calling is template-rendered XML; without
  the Jinja handler the calls never round-trip into OpenAI `tool_calls`.
- **`-c 8192`** — measured 8K vs 32K A/B (2026-09, corrected harness): overall
  score is flat (66.2 vs 67.9) but 8K wins tool accuracy 81%→70%, keeps
  parallel multi-tool calls (100%→33%), runs faster, and only loses memory
  recall (0% vs 20%) — which the platform-side extractor should restore.
  Take 8K.
- **`--temp 1.0 --top-p 0.95`** — model-card recommendation, used server-side
  as default; NeoMind's requests carry their own sampler (temp 0.6) which
  overrides it — both were validated working.
- **`--repeat-penalty 1.0`** — Mamba-style hybrids degrade under repeat
  penalty (vendor recommendation; verified in testing).
- **`-c 131072`** — the hybrid KV state is cheap; long agent loops on slow
  models legitimately grow past 20k tokens and truncation breaks multi-round
  tool flows.

Quantization: Q4_K_M is the sweet spot measured here. Q6_K was not observed to
matter (test aborted — effect below noise for the effort).

## Registering in NeoMind

1. **Agent backend (active)**: Settings → LLM Backends → add a
   **llama.cpp** backend pointing at the *text* server
   (`http://<host>:8081`, no `/v1` suffix — the llamacpp client appends its
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
