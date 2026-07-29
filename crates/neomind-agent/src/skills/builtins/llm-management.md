---
id: llm-management
name: LLM Backend Management
category: llm
origin: builtin
priority: 80
token_budget: 8000
triggers:
  keywords: [llm, LLM, 大模型, 语言模型, model, 模型, backend, 后端, ollama, openai, qwen, gpt, deepseek, llama, multimodal, 多模态, vision model, 视觉模型, llm create, llm list, llm activate, llm test, default backend, activate backend, switch backend, change model, api key, 端点, endpoint]
  tool_target:
    - tool: llm
      actions: [list, get, models, create, update, delete, activate, test]
anti_triggers:
  keywords: [device, 设备, rule, 规则, dashboard, 仪表盘, widget develop, extension develop]
---

# LLM Backend Management

## Command Cheat-Sheet (run these via `shell`)

Always RUN the command yourself and report the real output.

| Command | Purpose |
|---|---|
| `neomind llm list` | List configured LLM backends |
| `neomind llm get <id>` | LLM backend details |
| `neomind llm models` | List available models (from Ollama) |
| `neomind llm create` | Create a new LLM backend |
| `neomind llm update <id>` | Update LLM backend configuration |
| `neomind llm delete <id>` | Delete an LLM backend |
| `neomind llm activate <id>` | Activate a backend (set as default) |
| `neomind llm test <id>` | Test LLM backend connection |

LLM backends are the model providers that power all agents. Each agent is bound to one backend (via `--llm-backend <ID>`, or the system default if unset). Backends are **multi-instance** — you can register several and switch between them.

## CRITICAL: Create → Test → Activate Workflow

```bash
# Step 1: See what's already configured
neomind llm list

# Step 2: For Ollama, find available model names
neomind llm models                                # queries http://localhost:11434
neomind llm models --endpoint http://gpu-host:11434   # remote Ollama

# Step 3: Create the backend
neomind llm create --name local-qwen --type ollama \
  --endpoint http://localhost:11434 --model qwen3:8b

# Step 4: VERIFY connectivity before relying on it
neomind llm test <ID>            # returns latency + sample response

# Step 5: Set as system default (new agents will use it)
neomind llm activate <ID>
```

> **Never skip `llm test`** — a misconfigured endpoint or wrong API key will fail silently until an agent tries to execute.

## Backend Types

| `--type` | Endpoint | Auth | When to Use |
|----------|----------|------|-------------|
| `ollama` | `http://host:11434` | None (local) | Local/private deployment, GPU host, air-gapped environments |
| `openai` | `https://api.openai.com/v1` (or compatible) | `--api-key` (required) | Cloud GPT models, DashScope, DeepSeek, any OpenAI-compatible API |
| `custom` | Any URL | `--api-key` (optional) | Self-hosted vLLM, LM Studio, OpenRouter, etc. |

### OpenAI-Compatible Cloud Examples

```bash
# DashScope (Alibaba Qwen)
neomind llm create --name qwen-plus --type openai \
  --endpoint https://dashscope.aliyuncs.com/compatible-mode/v1 \
  --model qwen-plus --api-key sk-xxx

# DeepSeek
neomind llm create --name deepseek --type openai \
  --endpoint https://api.deepseek.com/v1 \
  --model deepseek-chat --api-key sk-xxx

# OpenAI GPT-4o
neomind llm create --name gpt4o --type openai \
  --endpoint https://api.openai.com/v1 \
  --model gpt-4o --api-key sk-xxx
```

## Multimodal (Vision) Capability

A backend can process images only if its `capabilities.multimodal` is `true`. Capability is resolved via a priority chain — **never assume** a model is vision-capable from its name alone:

| Priority | Source | Description |
|----------|--------|-------------|
| 1 (highest) | User override | `PATCH /api/llm-backends/:id/capabilities` with `{"multimodal": true/false}` — sacred, never auto-corrected |
| 2 | Runtime API | Ollama `/api/show` detection (hourly refresh) |
| 3 | LiteLLM registry | Embedded model registry (2748+ entries) |
| 4 | Heuristic | Conservative suffix match (`-vl`, `vision`, `qwen3.5-3.7`, `gpt-4o`, etc.) |
| 5 (lowest) | `false` | Fallback if nothing matches |

### Checking Multimodal Before Creating Vision Agents

```bash
neomind llm list
# Look at each backend's capabilities object:
#   "multimodal": true|false
#   "multimodal_source": "user_override" | "runtime_api" | "registry" | "heuristic"
#   "multimodal_user_override": true|false|null
```

If a vision-capable model reports `multimodal: false`, set an override:
```bash
# Via REST (CLI doesn't expose PATCH yet, use shell + curl)
curl -X PATCH http://localhost:9375/api/llm-backends/<ID>/capabilities \
  -H "Content-Type: application/json" \
  -d '{"multimodal": true}'
```

### Known Vision Model Families (Heuristic Covers These)
- **Qwen**: `qwen3-vl`, `qwen3.5-plus`, `qwen3.6-plus`, `qwen3.7-plus` (hybrid thinking), `qwen-vl-plus`
- **OpenAI**: `gpt-4o`, `gpt-4o-mini`, `gpt-4-turbo`, `o1` (except `o1-mini`)
- **Anthropic**: `claude-3-*`, `claude-4-*`
- **Google**: `gemini-1.5-*`, `gemini-2-*`
- **Open source**: `llava`, `moondream`, `minicpm-v`, `pixtral`, `cogvlm`, `internvl`, `yi-vl`, `glm-4v`, `glm-5v`
- **Suffix markers**: `-vl`, `:vl`, `_vl`, `-vision`, `_vision`, `vision`

> **DashScope note**: Qwen `qwen3.5-plus` / `qwen3.6-plus` / `qwen3.7-plus` ARE multimodal despite being listed in the text-gen doc. Always cross-check the 视觉推理 doc; Alibaba model categories overlap.

## Binding Backends to Agents

```bash
# Use system default backend (set via `llm activate`)
neomind agent create --name 'Temp Monitor' --prompt 'Check temperatures'

# Bind a specific backend (e.g., a vision model for a camera agent)
neomind agent create --name 'Camera Watch' \
  --prompt 'Analyze camera frames for anomalies' \
  --llm-backend <VISION_BACKEND_ID>
```

Find the backend ID with `neomind llm list` before creating the agent.

## Updating Without Breaking Agents

`llm update` changes the endpoint/model in place — agents referencing the backend pick up the new config on next execution. To safely swap:

```bash
# 1. Create the new backend
neomind llm create --name qwen-new --type ollama --endpoint http://localhost:11434 --model qwen3:32b
# 2. Test it
neomind llm test <NEW_ID>
# 3. Activate as default (optional — or migrate agents individually)
neomind llm activate <NEW_ID>
# 4. Update specific agents if needed
neomind agent update <AGENT_ID> --llm-backend <NEW_ID>
# 5. Retire old backend only after confirming agents work
neomind llm delete <OLD_ID>
```

> **Deleting a backend in use** causes those agents to fail on next execution. Always run `neomind agent list` and check `llm_backend_id` first.

## Thinking Models (qwen3.x, deepseek-r1)

Models that support thinking/reasoning consume extra tokens. For non-chat LLM calls (memory extraction, context compression), the platform auto-sets `thinking_enabled: false`. To force-disable thinking on a cloud OpenAI-compatible backend (DashScope), the request includes `enable_thinking: false`. This is handled automatically — no CLI flag needed.

If a thinking model causes gateway timeouts on long responses, consider:
1. Setting a smaller `max_rounds` on the agent
2. Switching to a non-thinking variant for that workload

## CLI Command Reference

| Command | Purpose |
|---------|---------|
| `neomind llm list` | List all backends with capabilities |
| `neomind llm get <ID>` | Full backend config details |
| `neomind llm models [--endpoint URL]` | List models from Ollama server |
| `neomind llm create --name N --type T --endpoint URL --model M [--api-key K] [--temperature 0.7]` | Register backend |
| `neomind llm update <ID> [--name N] [--model M] [--endpoint URL] [--api-key K] [--temperature F]` | Modify config |
| `neomind llm delete <ID>` | Remove backend (check agent usage first!) |
| `neomind llm activate <ID>` | Set as system default |
| `neomind llm test <ID>` | Verify connectivity + sample response |

## Common Errors & Solutions

| Symptom | Cause | Fix |
|---------|-------|-----|
| `llm test` returns connection refused | Ollama server not running | Start Ollama, check `--endpoint` URL |
| `llm test` returns 401 Unauthorized | Wrong/missing API key | `llm update <ID> --api-key sk-xxx` |
| `llm test` returns model not found | Model name typo or not pulled | Ollama: `ollama pull <model>`. Cloud: verify model name in provider docs |
| Agent fails with "multimodal not supported" | Bound backend is text-only | Check `llm list` capabilities, switch agent to a vision backend |
| Image input causes API 400 `unknown variant image_url` | Backend reports multimodal but model is text-only | Set user override: `PATCH .../capabilities {"multimodal": false}` |
| `llm create` says invalid type | Wrong `--type` value | Must be `ollama`, `openai`, or `custom` |
| Agent execution hangs / times out on thinking model | Long thinking chains on cloud backend | Reduce `max_rounds`, switch model, or set `thinking_enabled: false` via backend config |
| No backend is auto-used by new agents | No active default | Run `neomind llm activate <ID>` |

## Related Skills

- **agent-management** — how to bind backends to agents via `--llm-backend`
- **transform-management** — transforms run on the platform, not LLM backends (separate concern)
- **extension-development** — extensions are process-isolated and don't use LLM backends directly
