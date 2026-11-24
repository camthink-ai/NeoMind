---
id: agent-management
summary: Create and operate recurring or scheduled AI agents.
not_for: [one-off immediate device actions (use device control), threshold rules (rule-management)]
name: AI Agent Management
aliases: [agent, agents]
description: Use when the user wants to create, configure, or manage AI agents — schedules (cron/interval/event/manual), triggers, invoking/executing agents, agent memory, execution history, or monitoring agents. Covers agent create/list/update/delete/control/invoke/memory/executions even if they don't say 'agent' (e.g. '让助手每天定时跑', '安排一个监控任务', 'create a job', '建个AI job', 'make an AI task', '帮我盯着' — agents are also called jobs, AI jobs, AI tasks, or 盯盘/巡检任务). Includes 创建/配置/调用代理、定时任务、代理记忆.
category: agent
origin: builtin
priority: 85
token_budget: 10000
triggers:
  keywords: [agent, 代理, AI代理, agent create, 创建代理, agent control, schedule, cron, interval, manual, 监控, agent invoke, 调用代理, agent memory, 代理记忆, agent execution, 代理执行, 定时任务, scheduled task, agent schedule, agent update, 按需, 手动执行, job, jobs, ai job, AI job, AI任务, AI 任务, ai task, AI task, AI 任务, task, 巡检, 巡检任务, 盯盘, 盯着, 帮我盯, 帮我盯着, 定时检查, 定期检查, 例行检查, 自动检查, 每天检查, 定时跑, 跑个任务, 建个任务, 每天自动, 定期执行, keep an eye, check regularly, watch for, run daily, recurring task, set up a job, make a task, create a task, is it ok, is it okay, ok right now, okay right now, 还好吗, 还好么, 正常吗, 正常不, 怎么样了, 现在怎样, latest execution, 最新结论, query conclusion]
  tool_target:
    - tool: agent
      actions: [list, get, create, update, delete, control, invoke, memory, executions, latest-execution, conversation, send-message]
anti_triggers:
  keywords: [device, 设备, rule, 规则, dashboard, 仪表盘, extension develop]
---

# agent-management

**Status / "is it OK right now" questions** about something an agent already watches: `neomind agent list` → match by name → `neomind agent latest-execution <ID>` (via shell). Do **not** invent a reading from an empty `device list` / system-info, and do **not** `agent invoke` unless the user asked for a fresh recheck.
Read an existing conclusion with neomind agent latest-execution <ID> before starting a new run. Resolve names with agent list/get; use the intended agent, not a guessed ID.
Use neomind agent invoke <ID> [input] for an explicitly requested fresh execution. Input requirements depend on the agent's task and bindings. Executing means the existing run continues; query its result instead of invoking again.
Choose focused for one-pass analysis, free for tool-driven tasks, structured for schema-validated output. Structured agents require actual bound input; do not substitute memory for missing telemetry.
Choose manual for on-demand work, interval for elapsed periods, cron for local-time schedules, event for telemetry triggers. Cron must use the user's timezone; verify schedule and timezone on readback.
Creation can activate scheduled work immediately. Preserve scope, bindings and notification intent. For schema, bindings, event filters, notification routing and examples, load the relevant reference section.
After create/update, read back the configuration. Verify run records and conclusion before claiming the requested analysis completed.

## Commands
| Command | Purpose |
|---|---|
| `neomind agent list` | List all agents |
| `neomind agent get <id>` | Agent details |
| `neomind agent create` | Create a new agent |
| `neomind agent update <id>` | Update an agent |
| `neomind agent delete <id>` | Delete an agent |
| `neomind agent control <id> <active|paused>` | Control agent status |
| `neomind agent invoke <id> [input]` | Invoke / run an agent now with input |
| `neomind agent memory <id>` | Get agent memory |
| `neomind agent clear-memory <id>` | Clear agent memory |
| `neomind agent executions <id>` | Execution history |
| `neomind agent latest-execution <id>` | Latest execution |
| `neomind agent conversation <id>` | Get agent conversation / messages |
| `neomind agent send-message <id> <text>` | Send a message to an agent |

## Detailed reference
Use skill(action="load", id="agent-management", section="<exact returned title>") only for the needed detail. Section titles are returned with this overview.

<!-- SKILL_REFERENCE -->


# AI Agent Management

Agents are LLM-powered automated tasks. They can be scheduled (interval/cron) or event-driven, and have access to the shell tool to execute CLI commands.

## Command Cheat-Sheet (run these via `shell`)

All agent operations go through `shell`. Always RUN the command yourself and report the real output.

| Command | Purpose |
|---|---|
| `neomind agent list` | List all agents |
| `neomind agent get <id>` | Agent details |
| `neomind agent create` | Create a new agent |
| `neomind agent update <id>` | Update an agent |
| `neomind agent delete <id>` | Delete an agent |
| `neomind agent control <id> <active|paused>` | Control agent status |
| `neomind agent invoke <id> [input]` | Invoke / run an agent now with input |
| `neomind agent memory <id>` | Get agent memory |
| `neomind agent clear-memory <id>` | Clear agent memory |
| `neomind agent executions <id>` | Execution history |
| `neomind agent latest-execution <id>` | Latest execution |
| `neomind agent conversation <id>` | Get agent conversation / messages |
| `neomind agent send-message <id> <text>` | Send a message to an agent |

## CRITICAL: 4-step create SOP

New agents are created **active** and may run immediately. Follow these steps — never paste Chat history into `--prompt`.

1. **Discover real IDs first**: `device list` → `device get <ID>` (metric names/types). Never invent `device_id` / metric fields.
2. **Pick mode / trigger / notify / tools** from the profile table below. Hard rule: `--no-tools` conflicts with `--notify on=judgment` (judgment needs `--allowed-tools shell`). Use `--output-schema` only for `structured`.
3. **Isolate context**: distill a clean business `--prompt` (target, criteria, action/output, silence rules). Do not dump chat troubleshooting into the agent.
4. **Report the effective summary**: after create/update, read back mode, schedule, bindings, tool_config, notify — and surface any `warnings` from the response.

| Profile | Mode | Trigger | Notify | Tools | Bindings | Example |
|---|---|---|---|---|---|---|
| 1a pure analysis / always report | focused | event / interval / cron | always | `--no-tools` | required metrics/device-ids | status digest, describe every frame |
| 1b conditional alert | focused | event / interval | judgment | `--allowed-tools shell` | required metrics/device-ids | temp alert, vision anomaly→message |
| 2 structured label | structured | event / interval | failure | (none; L0) | metrics + `--output-schema` | risk enum / people count |
| 3 monitor + limited control | focused | event / interval | judgment or failure | `--allowed-tools shell` (+ whitelist) | metrics + `--commands` | overheat → shutdown + notify |
| 4 cross-device inspect | free | cron / interval | always or judgment | default (optional whitelist) | optional | daily patrol |

```bash
# 1b — focused + judgment + shell only (bind real IDs from device get)
neomind agent create --name 'Temp Alert' --prompt 'Alert only when temp > 30C' \
  --schedule-type event --execution-mode focused \
  --metrics '[{"device_id":"<real-id>","metric_name":"temperature","display_name":"Temp"}]' \
  --allowed-tools shell --notify '{"channels":["IM"],"on":"judgment"}'

# Pause / resume later
neomind agent control <id> paused
neomind agent control <id> active
```

## Schedule Types

| Type | `--schedule-type` | Shortcut / `--schedule-config` | Example |
|------|-------------------|-------------------------------|---------|
| Event | `event` (default) | optional `--event-filter` | Device uplink matching filter (no field = whole device) |
| Interval | `interval` | `--every 5m` or `--schedule-config '300'` | every 5 min |
| Cron | `cron` | `--schedule-config` + `--timezone` | `'0 9 * * *' --timezone 'Asia/Shanghai'` |
| Manual | `manual` | — | only via `agent invoke` |

Cron timezone is separate from the expression — pass `--timezone` (e.g. `Asia/Shanghai`) and verify on `agent get`. Manual agents show `Completed` between invokes (ready to run again). `--every 30s|5m|1h|2d` replaces interval+seconds.

## Execution Modes

| Mode | What happens | When to Use |
|------|--------------|-------------|
| `focused` (default) | Single-pass on bound data | summaries / checks — **requires** `--device-ids`/`--resources`/`--metrics`/`--commands` |
| `free` | Multi-round tools | open investigation; bindings optional |
| `structured` | One constrained inference → `ai:{id}:{field}` | schema labels; **requires** bindings + `--output-schema` |

Default focused without bindings → `FOCUSED_REQUIRES_RESOURCES`. Unbound create→invoke→delete → `--execution-mode free`.

### Output Contract (structured)

`--output-schema` declares the fields the agent must produce. Every field is
published as a live data source `ai:{agent_id}:{field}` — dashboards, rules
and data-push bind to it like any device metric.

```bash
neomind agent create --name '冷库巡检' --prompt '判断冷库状态' \
  --execution-mode structured --every 15m \
  --output-schema '[{"name":"status","field_type":{"type":"enum","values":["正常","异常"]},"description":"整体状态"},{"name":"score","field_type":{"type":"number"},"unit":"分"}]'
```

Field types: `number`, `text`, `boolean`, `{"type":"enum","values":[...]}`.
On structured agents the fields come from the single inference. On free/
focused agents the contract rides along: after the run, one extra step
renders the conclusion into the schema. Field NAMES become data-source keys
(ASCII recommended); descriptions localize.

### Memory axis

`--memory-mode tool` — judged fresh each run (cheapest, predictable; the
default for structured). `--memory-mode assistant` — carries the recent
run narrative ("worse than yesterday" needs this). Omit to derive from the
mode.

### Notification routing

`--notify` picks WHERE and WHEN — no more inferring from the conclusion
text:

```bash
neomind agent update 冷库巡检 --notify '{"channels":["IM","webhook:ops"],"on":"always"}'
```

Three triggers:

| `on` | Sends |
|---|---|
| `"failure"` (default) | only when the run failed — silence is health |
| `"always"` | every run, success or failure |
| `"judgment"` | **nothing.** The task decides for itself, in its own prompt, whether the operator needs to hear from it |

Channel names come from `neomind message channel-list`. Agents with **no**
notify config keep the legacy keyword behavior (the conclusion is scanned for
words like 异常/critical); once a config exists, that path is off — which is
what makes `"judgment"` mean silence rather than "silence plus a sniffed alert".

Reach for `"judgment"` when the user asks for something periodic but does not
want a message every time — "only tell me if it matters", "别每次都发", "让它自己判断".
The task gets the tools and the instruction to judge; you do not need to write
that logic. Do **not** combine it with `"always"` reasoning ("it should also
send every time") — pick one.

`IM` is a built-in channel — no setup, and always available. Reach for it
whenever the user asks to be told where they are already talking to you
("在 Telegram 告诉我", "给我发消息"), because it is the only channel that
reaches their chat: it delivers to every IM chat bound under Settings → IM
Bridges (the `/start` handshake). Without it, a run's outcome lands in the
in-app Messages page and nowhere else — a scheduled agent that reports only
in-app is indistinguishable from one that never ran.

**A new agent already has somewhere to report.** The API applies
`{"channels":["IM"],"on":"failure"}` to any create request that omits `notify` —
CLI, editor and API alike — so leaving `--notify` off does **not** mean "no
notification". The editor shows that floor on the form, and a user comparing the
two will notice if you promise otherwise. An agent stays quiet only when you set
`"on":"judgment"` (the task decides) or an explicit `{"channels":[]}` (routes
nowhere).

### Binding Resources

**Never invent these IDs.** The examples below are shapes, not values — a real
`sensor-001` may not exist, and an agent bound to a device that does not exist
collects nothing. Find them first:

- `neomind device list` — the real device IDs. It lists devices only; it does
  **not** carry metric names.
- `neomind device get <ID>` — one device's actual metric fields.
- `neomind device types list` — a type's `metric_fields`, for what a device of
  that type can report.

A Structured agent with no bound source **refuses to run** rather than publish
a guess, so binding the wrong (or no) source is an agent that never succeeds.
Confirm the IDs exist before creating.

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

## LLM backend & vision

`neomind llm list` → match `capabilities.multimodal` / `supports_images` / `function_calling` / `max_context`. Omit `--llm-backend` to use the active default. Vision agents need a multimodal backend; the `vision` tool accepts data URLs or HTTP image URLs and uses that backend. Overrides: **llm-management** skill.


## Command Reference

### Create Agent

```bash
neomind agent create \
  --name '<name>' --prompt '<task_description>' \
  [--schedule-type <event|interval|cron|manual>] [--schedule-config '<config>'] \
  [--event-filter '<json>'] [--timezone '<IANA>'] [--every <dur>] \
  [--execution-mode <focused|free|structured>] [--output-schema '<json>'] \
  [--notify '{"channels":["name"],"on":"failure|always|judgment"}'] \
  [--no-tools | --allowed-tools <list|all>] \
  [--device-ids 'id1,id2'] [--metrics '<json>'] [--commands '<json>'] [--resources '<json>']
```

**Required**: `--name`, `--prompt`. Tools: `--no-tools` XOR `--allowed-tools` (`all` restores full set). Never `--no-tools` + judgment notify. Match `--llm-backend` via `llm list` capabilities.

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
neomind agent update <ID> --prompt 'New task'
neomind agent update <ID> --event-filter '{"sources":[{"type":"device","id":"<id>"}]}' --timezone 'Asia/Shanghai'
neomind agent update <ID> --allowed-tools all   # reverse a prior --no-tools
neomind agent update <ID> --no-tools
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

**Whole device or one field:**
- **No `field`** = the agent listens to the WHOLE device. It runs once per uplink and receives that uplink as a whole: every metric the device reported in it, with images attached as images (not as text). Use this for camera and vision tasks ("watch this camera") and whenever the prompt needs more than one reading. You do not need to bind the device's metrics yourself.
- **With `field`** = the agent runs only when that metric is reported, and receives that metric's value plus whatever is bound with `--metrics` / `--resources`. Use it when the prompt is about one specific reading.
- After a run, `neomind agent latest-execution <AGENT_ID>` shows what the run was given (`input_summary`: metric names and attached images). Check it when a new event agent's conclusions look unfounded.

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
neomind agent create --name 'Health Check' --prompt 'Check all devices' \
  --every 10m --execution-mode free
# ... agent starts immediately, check results ...
neomind agent invoke <ID> "run once now"
neomind agent latest-execution <ID>
neomind agent delete <ID>   # required when the user asked to remove it — do not leave leftovers
```

## Common Errors & Solutions

| Error | Cause | Solution |
|-------|-------|----------|
| "Agent not found" | Wrong ID | Run `neomind agent list` for valid IDs |
| Create fails | Missing `--name` or `--prompt` | Both are required flags |
| Agent not running on schedule | Status is `paused` or wrong schedule config | Run `agent get <ID>` to check status and schedule, then `agent control <ID> active` if paused |
| Control fails | Invalid status value | Only `active` and `paused` are valid |
| Focused mode error | No resources bound (default mode) | Add `--device-ids` / `--resources`, **or** `--execution-mode free` |
| `unexpected argument '--input'` / `'--prompt'` on invoke | Input is positional | `neomind agent invoke <ID> "your prompt"` — not `--input` / `--prompt` (those flags are create/update only) |
| Execution shows error | LLM or tool failure | Check `agent conversation <ID>` for details |
| Bad LLM responses | Wrong model/backend | Run `neomind llm list` for available backends, update with `agent update <ID> --llm-backend <backend>` |
