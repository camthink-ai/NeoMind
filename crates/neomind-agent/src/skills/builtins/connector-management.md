---
id: connector-management
name: Connector Management & MQTT Configuration
category: connector
origin: builtin
priority: 80
token_budget: 8000
triggers:
  keywords: [connector, 连接器, MQTT, broker, 代理, 订阅, subscribe, topic, 主题, webhook, 外部, external, broker, 远程, remote, 订阅主题, connector create, connector test, subscription]
  tool_target:
    - tool: connector
      actions: [list, get, create, update, delete, test, subscriptions, subscribe, unsubscribe]
anti_triggers:
  keywords: [dashboard, 仪表盘, agent, 代理, extension develop, 扩展开发, rule, 规则]
---

# Connector Management & MQTT Configuration

Connectors link NeoMind to external MQTT brokers and data sources. They allow subscribing to topics and bridging data from remote systems.

## CRITICAL Rules

1. **Connectors are for EXTERNAL/remote brokers the user configures** — NOT for the built-in broker. If the user wants the **active broker address / port / network info** (e.g. to connect a device like ESP32 to NeoMind), run `neomind system info` (see `system-info` skill) — do NOT run `connector list` or OS commands (`ipconfig`/`ip a`) for that.
2. **Always test connection after creating** — use `connector test <ID>`
3. **Default port is 1883** (non-TLS) or 8883 (TLS)
4. **`broker` is a deprecated alias** — use `connector` instead
5. **Topics are comma-separated** — e.g., `--topics "sensors/temp,sensors/humidity"`

## Command Reference

### Create Connector

```bash
neomind connector create --name <NAME> --host <HOST> [--port <PORT>] [options]
```

| Flag | Required | Default | Description |
|------|----------|---------|-------------|
| `--name` | Yes | — | Connector display name |
| `--host` | Yes | — | Broker hostname or IP |
| `--port` | No | 1883 | Broker port |
| `--connector-type` | No | mqtt | Connector type (mqtt, webhook, http) |
| `--tls` | No | false | Enable TLS (flag, no value) |
| `--username` | No | — | Auth username |
| `--password` | No | — | Auth password |
| `--topics` | No | — | Comma-separated topics to auto-subscribe |

### List & Get

```bash
neomind connector list                     # List all connectors
neomind connector get <ID>                 # Get connector details
```

### Test Connection

```bash
neomind connector test <ID>                # Test if connection works
```

### Update & Delete

```bash
neomind connector update <ID> [--host <H>] [--port <P>]  # Update connector
neomind connector delete <ID>                              # Delete connector
```

Update flags: `--name`, `--host`, `--port`, `--tls=true|false`, `--username`, `--password`, `--topics`, `--disable=true|false`

### Enable / Disable (unified form, preferred over `update --disable`)

```bash
neomind connector enable <ID>     # Re-enable a disabled connector
neomind connector disable <ID>    # Stop connection without deleting
```

### Subscription Management

```bash
neomind connector subscriptions            # List all MQTT subscriptions
neomind connector subscribe --topic <TOPIC> [--qos <0|1|2>]   # Subscribe
neomind connector unsubscribe --topic <TOPIC>                  # Unsubscribe
```

## Workflow Examples

### Connect to Remote MQTT Broker

```bash
# Step 1: Create connector
neomind connector create --name 'Factory Broker' \
  --host 192.168.1.100 \
  --port 1883

# Step 2: Test connection (record the ID from step 1)
neomind connector test <ID>

# Step 3: Subscribe to topics
neomind connector subscribe --topic 'factory/sensors/#'
```

### Secure TLS Connection

```bash
neomind connector create --name 'Cloud Broker' \
  --host broker.example.com \
  --port 8883 \
  --tls \
  --username myuser \
  --password mypass \
  --topics "devices/telemetry,devices/status"
```

### Subscribe to Specific Topics

```bash
# List current subscriptions
neomind connector subscriptions

# Subscribe to a topic
neomind connector subscribe --topic 'home/temperature' --qos 1

# Unsubscribe
neomind connector unsubscribe --topic 'home/temperature'
```

### Update Connector Settings

```bash
# Change host/port
neomind connector update <ID> --host new-broker.local --port 1883

# Disable connector (preferred unified form)
neomind connector disable <ID>

# Re-enable
neomind connector enable <ID>
```

## Common Errors & Solutions

| Error | Cause | Solution |
|-------|-------|----------|
| "Connection refused" | Wrong host/port or broker offline | Verify broker is running and port is correct |
| "Connection timeout" | Firewall or network issue | Check network connectivity, try `ping <host>` |
| "Auth failed" | Wrong username/password | Verify credentials with `connector update <ID> --username --password` |
| "TLS handshake failed" | TLS on non-TLS port | Remove `--tls` flag or use correct TLS port (usually 8883) |
| Topics not receiving data | Not subscribed | Run `connector subscriptions` to check, then `connector subscribe --topic <T>` |
| "Connector not found" | Wrong ID | Run `connector list` for valid IDs |
