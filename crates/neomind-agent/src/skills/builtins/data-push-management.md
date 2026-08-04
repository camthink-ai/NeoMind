---
id: data-push-management
name: Data Push Management & Target Configuration
description: Use when the user wants to forward or export data to external systems — webhook/MQTT push targets, push schedules, delivery history, test pushes. Covers push create/update/test/logs even without saying 'push' (e.g. '把数据转发到我的服务器', '数据导出'). Includes 数据推送/转发/导出/webhook 推送.
category: push
origin: builtin
priority: 75
token_budget: 6000
triggers:
  keywords: [push, 推送, data push, 数据推送, push target, 推送目标, webhook push, mqtt push, external, 外部, forward, 转发, export data, 数据导出, push data, 推数据, 数据推送, delivery history, 推送历史]
  tool_target:
    - tool: push
      actions: [list, get, create, update, delete, start, stop, test, logs, stats]
anti_triggers:
  keywords: [dashboard, 仪表盘, agent, 代理, extension develop, 扩展开发, rule, 规则, message send, 发消息, channel, 通道, notification, 通知]
---

# Data Push Management & Target Configuration

Data Push forwards device metrics and extension outputs to external systems (webhook, MQTT broker) in real-time or on a schedule.

## Command Cheat-Sheet (run these via `shell`)

Always RUN the command yourself and report the real output. The CLI group is `neomind push` (not `data-push`).

| Command | Purpose |
|---|---|
| `neomind push list` | List push targets |
| `neomind push get <id>` | Push target details |
| `neomind push create` | Create a push target |
| `neomind push update <id>` | Update a push target |
| `neomind push delete <id>` | Delete a push target |
| `neomind push enable <id>` | Enable a target (alias for `push start`) |
| `neomind push disable <id>` | Disable a target (alias for `push stop`) |
| `neomind push test <id>` | Test a push target |
| `neomind push logs <id>` | Delivery logs for a target |
| `neomind push stats` | Push statistics |

## CRITICAL Rules

1. **`--name` is required** for creating push targets
2. **`--type` is required**: `webhook` or `mqtt`
3. **`--config` must be valid JSON** with type-specific fields
4. **Targets start in stopped state** — must `push enable <ID>` after creation
5. **Always test** with `push test <ID>` before relying on delivery
6. **Use `--sources`** to filter which data gets forwarded (comma-separated patterns)

## Command Reference

### List & Inspect Targets

```bash
neomind push list                                # List all push targets
neomind push get <ID>                            # Get target details
neomind push stats                               # Overall push statistics
neomind push logs <ID> [--limit 20]              # Delivery history for a target
```

### Create Push Target

```bash
neomind push create --name <NAME> --type <TYPE> --config '<JSON>' [--schedule <SCHEDULE>] [--sources <PATTERNS>]
```

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--name` | Yes | — | Target name (unique identifier) |
| `--type` | Yes | webhook | Target type: `webhook` or `mqtt` |
| `--config` | Yes | — | Type-specific config as JSON |
| `--schedule` | No | event | Schedule: `event` (real-time) or `interval` (every 60s) |
| `--sources` | No | (all) | Comma-separated source patterns to filter |

### Manage Targets

```bash
neomind push enable <ID>                         # Start a stopped target (unified form)
neomind push disable <ID>                        # Stop a running target (unified form)
neomind push start <ID>                          # Legacy alias for enable (hidden, backward compat)
neomind push stop <ID>                           # Legacy alias for disable (hidden, backward compat)
neomind push test <ID>                           # Test delivery (sends sample data)
neomind push update <ID> [--name <N>] [--config '<JSON>'] [--enabled true|false]
neomind push delete <ID>                         # Delete a target
```

## Target Types & Config

### Webhook Target

Pushes data as HTTP POST JSON to an external URL.

```bash
neomind push create --name my-webhook --type webhook \
  --config '{"url": "https://example.com/api/data"}' \
  --schedule event \
  --sources "device:sensor-001:temperature,extension:weather:temp"
```

**Config fields:**
- `url` (required): Target endpoint URL
- `headers` (optional): Custom HTTP headers (e.g., `{"Authorization": "Bearer TOKEN"}`)
- `timeout_secs` (optional, default 30): Request timeout

### MQTT Target

Publishes data to an external MQTT broker.

```bash
neomind push create --name my-mqtt --type mqtt \
  --config '{"broker": "tcp://broker.example.com:1883", "topic": "neomind/data"}' \
  --schedule event
```

**Config fields:**
- `broker` (required): MQTT broker URL (e.g., `tcp://host:1883`)
- `topic` (required): MQTT topic to publish to
- `username` (optional): Auth username
- `password` (optional): Auth password
- `qos` (optional, default 0): MQTT QoS level (0, 1, 2)

## Schedule Types

### Event-Driven (default)

Forwards data immediately when new metrics arrive.

```bash
--schedule event
```

### Interval

Forwards data at regular intervals (aggregated).

```bash
--schedule interval
```

Default interval is 60 seconds.

## Source Filtering

Use `--sources` to filter which data sources get forwarded. Format: `{type}:{id}:{field}` (DataSourceId format).

```bash
# Only push temperature from specific device
--sources "device:sensor-001:temperature"

# Push multiple sources
--sources "device:sensor-001:temperature,device:sensor-001:humidity"

# Push all data from an extension
--sources "extension:weather:*"

# Push everything (default, omit --sources)
```

## Workflow Examples

### Forward Device Data to External Webhook

```bash
# Step 1: Create the target
neomind push create --name sensor-webhook --type webhook \
  --config '{"url": "https://myapp.example.com/api/neomind-data", "headers": {"Authorization": "Bearer my-token"}}' \
  --sources "device:sensor-001:temperature"

# Step 2: Start the target
neomind push enable <ID>

# Step 3: Test delivery
neomind push test <ID>

# Step 4: Check delivery logs
neomind push logs <ID>
```

### Push All Metrics to External MQTT Broker

```bash
# Create MQTT target
neomind push create --name cloud-mqtt --type mqtt \
  --config '{"broker": "tcp://mqtt.cloud.example.com:1883", "topic": "neomind/telemetry", "username": "user", "password": "pass", "qos": 1}' \
  --schedule event

# Start and test
neomind push enable <ID>
neomind push test <ID>
```

### Check Push Status & History

```bash
# Overall statistics
neomind push stats

# Delivery logs for a specific target
neomind push logs <ID> --limit 10

# Get target details (includes status, last delivery time)
neomind push get <ID>
```

### Update a Target

```bash
# Update config
neomind push update <ID> --config '{"url": "https://new-endpoint.example.com/data"}'

# Rename
neomind push update <ID> --name "renamed-target"

# Disable without deleting
neomind push update <ID> --enabled false
```

## Common Errors & Solutions

| Error | Cause | Solution |
|-------|-------|----------|
| "Target name is required" | Missing --name | Add `--name <NAME>` |
| "Unknown target type 'http'" | Invalid type | Use `webhook` or `mqtt` |
| "Invalid config JSON" | Malformed JSON | Validate syntax. webhook needs `{"url":"..."}`, mqtt needs `{"broker":"...","topic":"..."}` |
| "Target not found" | Wrong ID | Run `push list` for valid IDs |
| "Connection refused" | Target URL/broker unreachable | Verify URL, check network, use `push test` |
| "Timeout" | Slow external endpoint | Increase `timeout_secs` in config via `push update` |
| "Not started" | Target is stopped | Run `push enable <ID>` first |
| "No data forwarded" | Source filter too restrictive | Check `--sources` patterns, try without filter |
