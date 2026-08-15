# Edge Model Deployment Guide — LFM2.5 Dual-Model Recipe

NeoMind runs best on a single edge box with **two small models instead of one**:
a text model that *acts* (drives the agent's tool calling) and a vision model
that *sees* (describes images for the `vision` tool). This guide documents the
measured recipe using LiquidAI's LFM2.5 family, including the exact
llama.cpp flags that matter.

> All numbers below are hard-signal measurements (`cmd_ok` = exact CLI command
> emitted) from NeoMind's own 154-case bilingual eval, 2026-08.

## The split

| Role | Model | Measured (hard cmd_ok) | Why |
|---|---|---|---|
| **Agent** (tool calling) | `LFM2.5-2.6B` (text) | **64% full-en / 83% 30-case regression** — on par with Qwen3.5-4B | Mamba-hybrid: ~80 tok/s generation, ~800 tok/s prompt ingest on an M4 Pro; 1.6 GB (Q4_K_M) |
| **Perception** (vision) | `LFM2.5-VL-3B` (vision) | 10% as an agent — **do not use it as the agent** | Strong vision (ScreenSpot 80.7, OCR-class benchmarks), but the vision training materially degraded its tool calling despite sharing the 2.6B backbone |

Both models speak OpenAI-compatible function calling through llama.cpp's
`--jinja` chat-template path, verified end-to-end against NeoMind's agent loop.

## Serving (llama.cpp)

```bash
# Agent — LFM2.5-2.6B (text)
llama-server -m LFM2.5-2.6B-Q4_K_M.gguf \
  --host 127.0.0.1 --port 8081 -ngl 99 -c 131072 \
  --jinja --repeat-penalty 1.0 --top-k 50

# Perception — LFM2.5-VL-3B (vision; needs its mmproj)
llama-server -m LFM2.5-VL-3B-Q4_K_M.gguf --mmproj mmproj-LFM2.5-VL-3B-F16.gguf \
  --host 127.0.0.1 --port 8082 -ngl 99 -c 131072 \
  --jinja --repeat-penalty 1.0 --top-k 50
```

Non-negotiable flags:

- **`--jinja`** — LFM's function calling uses special-token-delimited calls;
  without the full Jinja chat-template handler the calls never round-trip into
  OpenAI `tool_calls` and every agentic request fails.
- **`--repeat-penalty 1.0`** — Mamba-style hybrids degrade under repeat
  penalty (vendor recommendation; verified in testing).
- **`-c 131072`** — the hybrid KV state is cheap; long agent loops on slow
  models legitimately grow past 20k tokens and truncation breaks multi-round
  tool flows.

Quantization: Q4_K_M is the sweet spot measured here. Q6_K was not observed to
matter (test aborted — effect below noise for the effort).

## Registering in NeoMind

1. **Agent backend (active)**: Settings → LLM Backends → add an
   OpenAI-compatible backend pointing at the *text* server
   (`http://<host>:8081`, no `/v1` suffix — NeoMind appends it), then activate
   it. This is the model that drives chat and scheduled agents.
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

LFM2.5 models are **`lfm1.0` (Liquid AI proprietary)** — NeoMind cannot bundle
them in the Docker image or installer. Users download the GGUFs themselves
([LFM2.5-2.6B-GGUF](https://huggingface.co/LiquidAI/LFM2.5-2.6B-GGUF),
[LFM2.5-VL-3B-GGUF](https://huggingface.co/LiquidAI/LFM2.5-VL-3B-GGUF)) and
serve them locally. For a bundlable default, the Docker image ships
Gemma4-E2B.

## Troubleshooting

| Symptom | Cause / fix |
|---|---|
| Every tool call fails, model produces plain text | Missing `--jinja` on the server |
| 404 on every request from NeoMind | Endpoint was given with `/v1` — remove it (NeoMind appends `/v1` itself) |
| Agent picks `skill`/wrong tool constantly on a ≤3B model | You are on an old NeoMind build with the bloated `shell` description — upgrade (fixed 2026-08; small models avoid huge tool descriptions) |
| Long multi-step deploys die mid-run | Chat turn bound was raised to 2400s; if you run a custom harness, make sure *its* per-turn and per-case budgets exceed the model's realistic completion time (~20+ min for 20-round deploys at edge speeds) |
| Vision works in isolation but agent never "sees" images | The active (text) backend not being multimodal is fine for tool-routed vision, but *user-uploaded chat images* currently require a multimodal active backend — upload via the vision flow instead |
