---
id: agent-management
name: AI Agent Management
description: Use when the user wants to create, configure, or manage AI agents — schedules (cron/interval/event/manual), triggers, invoking/executing agents, agent memory, execution history, or monitoring agents. Covers agent create/list/update/delete/control/invoke/memory/executions even if they don't say 'agent' (e.g. '让助手每天定时跑', '安排一个监控任务'). Includes 创建/配置/调用代理、定时任务、代理记忆.
category: agent
origin: builtin
priority: 85
token_budget: 10000
triggers:
  keywords: [agent, 代理, AI代理, agent create, 创建代理, agent control, schedule, cron, interval, manual, 监控, agent invoke, 调用代理, agent memory, 代理记忆, agent execution, 代理执行, 定时任务, scheduled task, agent schedule, agent update, 按需, 手动执行]
  tool_target:
    - tool: agent
      actions: [list, get, create, update, delete, control, invoke, memory, executions, latest-execution, conversation, send-message]
anti_triggers:
  keywords: [device, 设备, rule, 规则, dashboard, 仪表盘, extension develop]
---

# AI Agent Management

Agents are LLM-powered automated tasks. They can be scheduled (interval/cron) or event-driven, and have access to the shell tool to execute CLI commands.

## Command Cheat-Sheet (run these via `shell`)

Always RUN the command yourself and report the real output.

| Command | Purpose |
|---|---|
| `neomind agent list` | List all agents |
| `neomind agent get <id>` | Agent details |
| `neomind agent create` | Create a new agent |
| `neomind agent update <id>` | Update an agent |
| `neomind agent delete <id>` | Delete an agent |
| `neomind agent control <id> <pause|resume>` | Control agent status |
| `neomind agent invoke <id> [input]` | Invoke / run an agent now with input |
| `neomind agent memory <id>` | Get agent memory |
| `neomind agent clear-memory <id>` | Clear agent memory |
| `neomind agent executions <id>` | Execution history |
| `neomind agent latest-execution <id>` | Latest execution |
| `neomind agent conversation <id>` | Get agent conversation / messages |
| `neomind agent send-message <id> <text>` | Send a message to an agent |

## CRITICAL: Create → Active Pattern

New agents are created in **active** state and start executing immediately (if scheduled). Use this pattern:

```bash
neomind agent create --name 'Monitor' --prompt 'Check battery levels'
# → Returns agent ID (e.g., agent-abc123), already active and running

# To pause later:
neomind agent control agent-abc123 paused

# To resume:
neomind agent control agent-abc123 active
```

## Schedule Types

| Type | `--schedule-type` | Shortcut / `--schedule-config` | Example |
|------|-------------------|-------------------------------|---------|
| Event | `event` (default) | Not needed | Triggered by device data matching the event filter |
| Interval | `interval` | `--every 5m` (shortcut) or `--schedule-config '300'` | `--every 5m` = every 5 min |
| Cron | `cron` | `--schedule-config` | `--schedule-config '0 9 * * *'` = daily 9 AM |
| Manual | `manual` | Not needed | Never auto-scheduled — runs ONLY via `agent invoke` |

**Manual agents** are repeatable one-off workers: they never fire on their own, `agent invoke <ID>` runs them any number of times, and between runs they show status `Completed` (a ready-state — Completed agents can always be invoked again). Use manual for on-demand analysis, ad-hoc checks, or tasks delegated from chat.

**`--every` shortcut**: `--every 30s`, `--every 5m`, `--every 1h`, `--every 2d` — replaces `--schedule-type interval --schedule-config <seconds>`.

## Execution Modes

| Mode | Description | When to Use |
|------|-------------|-------------|
| `free` (default) | No bound resources, agent has full platform access | General monitoring, analysis tasks |
| `focused` | Bound to specific devices/rules | Requires `--device-ids` or `--resources` |

### Binding Resources (Focused Mode)

**`--device-ids`** (simple): comma-separated device IDs.
```bash
--device-ids 'sensor-001,sensor-002'
```

**`--resources`** (unified, preferred): JSON array with rich metadata.
```json
[
  {"resource_id": "sensor-001", "resource_type": "device", "name": "Living Room Sensor"},
  {"resource_id": "weather", "resource_type": "extension", "name": "Weather Service"}
]
```

**`--metrics`**: Bind specific metrics for the agent to monitor.
```json
[
  {"device_id": "sensor-001", "metric_name": "temperature", "display_name": "Temperature"},
  {"device_id": "sensor-001", "metric_name": "humidity", "display_name": "Humidity"}
]
```

**`--commands`**: Bind specific device commands the agent can execute.
```json
[
  {"device_id": "ac-unit", "command_name": "turn_on", "display_name": "Turn On AC"},
  {"device_id": "ac-unit", "command_name": "set_temp", "display_name": "Set Temperature", "parameters": {"target": 25}}
]
```

## Choosing the Right LLM Backend

Before creating an agent, check available backends and their capabilities:

```bash
neomind llm list
```

The response includes a `capabilities` object per backend. Match it to your task:

| Task Type | Required Capability | How to Check |
|-----------|-------------------|--------------|
| Text-only (monitoring, alerts, reports) | None (any backend works) | — |
| Vision / Image analysis | `multimodal: true` or `supports_images: true` | Look for vision models (e.g., `qwen3-vl`, `gpt-4o`) |
| Tool calling / function calling | `function_calling: true` | Most modern models support this |
| Long context tasks | `max_context` value | Compare context window sizes |

> If `--llm-backend` is not specified, the **default active backend** is used.

**Example — creating a vision-capable agent:**
```bash
# Step 1: Find a backend with multimodal support
neomind llm list
# Look for backends where capabilities.multimodal or capabilities.supports_images is true

# Step 2: Create agent with that backend
neomind agent create \
  --name 'Image Analyzer' \
  --prompt 'Analyze device camera images and detect anomalies' \
  --llm-backend <multimodal_backend_id>
```

### Vision Tool (Image Analysis)

Agents have access to a `vision` tool that can analyze images:
- **Data URLs**: `data:image/jpeg;base64,...` (preferred for inline images)
- **HTTP URLs**: any publicly accessible image URL (10s fetch timeout)

The tool uses the agent's bound LLM backend — the backend MUST have `capabilities.multimodal: true`. The tool returns structured text descriptions suitable for downstream reasoning, alerting, or reporting.

Typical use cases:
- Analyze device camera snapshots (via webhook/metric attachment)
- Inspect uploaded screenshots or diagnostic photos
- Verify visual state changes detected by sensors

For backend selection and multimodal capability overrides, see the **llm-management** skill.


## Command Reference

### Create Agent

```bash
neomind agent create \
  --name '<name>' \
  --prompt '<task_description>' \
  [--schedule-type <event|interval|cron|manual>] \
  [--schedule-config '<config>'] \
  [--description '<desc>'] \
  [--llm-backend '<llm_backend_id>'] \
  [--system-prompt '<instructions>'] \
  [--execution-mode <free|focused>] \
  [--device-ids 'id1,id2']
```

**Required**: `--name`, `--prompt`
**Important**: `--llm-backend` selects which LLM powers the agent. Run `neomind llm list` first and match backend capabilities to your task (see "Choosing the Right LLM Backend" above).

### Control Agent

```bash
neomind agent control <ID> active    # Start (positional status)
neomind agent control <ID> paused    # Stop
```

### Invoke (One-shot Execution)

```bash
neomind agent invoke <ID> 'Analyze current temperature sensors'    # Positional input
```

### Get Details & Status

```bash
neomind agent get <ID>          # Full config + status
neomind agent list              # All agents
```

### Update Agent

```bash
neomind agent update <ID> --prompt 'New task description'
neomind agent update <ID> --llm-backend qwen3.5:4b
neomind agent update <ID> --name 'Better Name' --description 'Updated'
```

### Monitor Executions

```bash
neomind agent executions <ID> --limit 10      # Execution history
neomind agent latest-execution <ID>           # Most recent execution
neomind agent conversation <ID> --limit 20    # Full message log
neomind agent memory <ID>                     # Execution journal + knowledge files
```

### Send Message

```bash
neomind agent send-message <ID> 'Focus on building A sensors'    # Positional message
neomind agent send-message <ID> 'Directive' --message-type instruction
```

## Workflows

### Interval-Based Monitoring Agent

```bash
# 1. Create agent that runs every 5 minutes (active immediately)
neomind agent create \
  --name 'Battery Monitor' \
  --prompt 'Check all devices battery levels. List devices below 20%. Send warning if any found.' \
  --every 5m

# 2. Check results after a few minutes
neomind agent latest-execution <AGENT_ID>
```

### Cron-Based Daily Report

```bash
# Daily at 9:00 AM (active immediately)
neomind agent create \
  --name 'Morning Report' \
  --prompt 'Summarize all device statuses. Count online/offline. Report anomalies from last 24 hours.' \
  --schedule-type cron \
  --schedule-config '0 9 * * *'
```

### On-Demand Analysis Agent

```bash
# No schedule — runs when invoked
neomind agent create \
  --name 'Device Analyzer' \
  --prompt 'Analyze the provided input and generate a detailed report.'

# Run whenever needed
neomind agent invoke <AGENT_ID> 'Analyze temperature trends for sensor-001'
```

### Event-Driven Agent (Triggered by Device Data)

Event agents execute automatically when device data arrives. Use `--event-filter` with a JSON string to limit which data sources trigger execution.

```bash
# Runs when ANY device data arrives (no filter = all bound resources)
neomind agent create \
  --name 'Data Logger' \
  --prompt 'Log incoming device data and check for anomalies.' \
  --schedule-type event

# Runs only when specific devices report (with filter)
neomind agent create \
  --name 'Temp Guardian' \
  --prompt 'Analyze temperature readings. Alert if any sensor reports above 40°C.' \
  --schedule-type event \
  --event-filter '{"sources":[{"type":"device","id":"sensor-001"},{"type":"device","id":"sensor-002"}]}'

# Trigger on ANY device of a type (use "all" for id)
--event-filter '{"sources":[{"type":"device","id":"all"}]}'

# Trigger on a specific metric field
--event-filter '{"sources":[{"type":"device","id":"sensor-001","field":"temperature"}]}'

# Trigger on extension data
--event-filter '{"sources":[{"type":"extension","id":"weather"}]}'
```

**When to use event vs interval:**
- **Event**: React immediately when data arrives (real-time anomaly detection)
- **Interval**: Check periodically regardless of data flow (periodic reports, summaries)
- **Without `--event-filter`**: Event agents without a filter trigger on data from their bound resources (focused mode). Free-mode event agents without a filter will NOT trigger automatically.

### Focused Mode Agent (Bound to Specific Devices)

```bash
# Create agent that only has access to specific devices
neomind agent create \
  --name 'Sensor Monitor' \
  --prompt 'Monitor temperature and humidity sensors. Alert if any reading is abnormal.' \
  --every 5m \
  --execution-mode focused \
  --device-ids 'sensor-001,sensor-002,sensor-003'
```

### Debug Agent Issues

```bash
# 1. Check status and config
neomind agent get <ID>

# 2. See recent execution results (check status, duration, error)
neomind agent latest-execution <ID>

# 3. If latest execution failed, check full conversation to see what happened
neomind agent conversation <ID> --limit 20

# 4. Check if LLM backend is available
neomind llm list
# If the configured model is not available, update:
neomind agent update <ID> --llm-backend <available_backend>

# 5. Check agent memory (execution journal + knowledge files)
neomind agent memory <ID>

# 6. If agent is stuck in a loop, pause and review
neomind agent control <ID> paused
neomind agent conversation <ID> --limit 50

# 7. After fixing, re-activate
neomind agent control <ID> active
```

### Full Lifecycle

```bash
neomind agent create --name 'Health Check' --prompt 'Check all devices' --every 10m
# ... agent starts immediately, check results ...
neomind agent latest-execution <ID>
neomind agent control <ID> paused    # Stop when done
neomind agent delete <ID>                      # Remove when no longer needed
```

## Common Errors & Solutions

| Error | Cause | Solution |
|-------|-------|----------|
| "Agent not found" | Wrong ID | Run `neomind agent list` for valid IDs |
| Create fails | Missing `--name` or `--prompt` | Both are required flags |
| Agent not running on schedule | Status is `paused` or wrong schedule config | Run `agent get <ID>` to check status and schedule, then `agent control <ID> active` if paused |
| Control fails | Invalid status value | Only `active` and `paused` are valid |
| Focused mode error | No resources bound | Add `--device-ids` or `--resources` |
| Execution shows error | LLM or tool failure | Check `agent conversation <ID>` for details |
| Bad LLM responses | Wrong model/backend | Run `neomind llm list` for available backends, update with `agent update <ID> --llm-backend <backend>` |
