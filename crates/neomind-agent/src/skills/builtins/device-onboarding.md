---
id: device-onboarding
name: Device Onboarding & Connection Guide
category: device
origin: builtin
priority: 90
token_budget: 12000
triggers:
  keywords: [设备接入, 接入, onboarding, 连接设备, connect device, MQTT, mqtt, broker, webhook, 传感器, sensor, 如何连接, how to connect, 怎么接入, 设备配置, device setup, device connect, 设备上线, provision, 配置设备, device provisioning, 网关, gateway, 接入方式, connection method, 接入协议, protocol, broker地址, broker address, 服务器地址, server address, topic, 主题, 订阅, subscribe, 发布, publish, draft, 草稿, 待审批, pending device, auto-discovery, 自动发现, 控制, 命令, 下发, 停机, 停止, 启动, 重启, 调速, 设置, 告警, 关, 开, control, command, send command, stop, shutdown, stop it, stop the, reboot, restart, set speed, set_speed, set alarm, turn off, turn on, enable, disable, calibrate, 传感器控制, device control]
  tool_target:
    - tool: system
      actions: [info]
    - tool: device
      actions: [create, list, types, control, latest, drafts, webhook-url, update, delete, history, write-metric]
anti_triggers:
  keywords: [rule, 规则, agent, 代理, dashboard, 仪表盘, transform, 转换]
---

# Device Onboarding & Connection Guide

## Command Cheat-Sheet (run these via `shell`)

Always RUN the command yourself and report the real output — don't narrate.

| Command | Purpose |
|---|---|
| `neomind device list` | List all devices (grouped by type) |
| `neomind device get <id>` | Details: metadata + metrics + commands |
| `neomind device create --name <N> --device-type <TYPE> [--id <id>] [--adapter-type <a>]` | Create a device |
| `neomind device update <id>` | Update a device |
| `neomind device delete <id>` | Delete a device |
| `neomind device history <id>` | Telemetry history |
| `neomind device control <id> <COMMAND>` | Send a control command to a device |
| `neomind device types list` | List device-type templates |
| `neomind device write-metric <id> <METRIC> <VALUE>` | Write a metric data point |
| `neomind device webhook-url <id>` | Get the webhook URL for a device |
| `neomind device drafts` | Manage auto-discovery drafts |

## Sending Control Commands

To send a command to a device (stop, set speed, alarm, reboot, ...):

1. **Always inspect first**: `neomind device get <id>` — its output lists the device's supported commands. Do NOT guess a command name.
2. **Send with the correct argument order**: `neomind device control <id> <COMMAND>` — the device ID comes FIRST, the command name SECOND. (Common mistake: writing the command before the ID, or `device <command> <id>`. The subcommand is `control`, not the command name.)
3. Optional params: `neomind device control <id> <COMMAND> --params '{"key":"value"}'`.

Example:
```bash
neomind device get pump-A            # → shows stop / set_speed / calibrate
neomind device control pump-A stop   # → sends stop (ID first, command second)
```

If `device get` shows the command is NOT defined for the device's template, tell the user it isn't available — do not fabricate a success.

## Overview

NeoMind supports multiple ways to connect IoT devices. Before helping users, **always check current infrastructure**:

```bash
neomind system info
```

This returns:
- **MQTT broker address**, protocol (`mqtt://` or `mqtts://`), and connection status
- **TLS status** (`tls_enabled`), **auth status** (`auth_enabled`), and **credentials** if auth is enabled
- **Webhook URL** for HTTP-based devices
- **Network info** (server IP, WiFi SSID)
- **Device connection details** (topics, payload formats)

To see all configured external brokers and their connection status:

```bash
neomind connector list
```

To see all active MQTT topic subscriptions:

```bash
neomind connector subscriptions
```

---

## Connection Methods

### Method 1: MQTT (Recommended)

MQTT is the primary device connection protocol. NeoMind includes an **embedded MQTT broker** — no external broker needed.

**Broker Info** (from `neomind system info`):
- Address: `mqtt://<SERVER_IP>:1883` (or `mqtts://` if TLS enabled)
- Protocol: MQTT 3.1.1
- Authentication: check `auth_enabled` in system info output
- TLS: check `tls_enabled` in system info output
- Credentials: listed in `credentials` array if auth is enabled
- Auto-discovery enabled

> **IMPORTANT**: Always run `neomind system info` first to get the actual broker URL, TLS, auth status, and credentials. The values above are defaults and may differ on the user's system.

**How devices connect:**

1. Device connects to MQTT broker at `<SERVER_IP>:1883`
2. Device publishes data to **any topic**
3. NeoMind auto-discovers the device and creates a draft entry
4. User approves the device in "Pending Devices"

**MQTT Topic Format:**

Devices can publish to any topic. Common patterns:
```
devices/{device_id}/temperature
devices/{device_id}/humidity
sensors/{sensor_id}/{metric_name}
```

**MQTT Payload Format:**

Simple JSON:
```json
{"value": 23.5}
```

Or with metadata:
```json
{
  "temperature": 23.5,
  "humidity": 65.0,
  "timestamp": 1716200000
}
```

**Auto-Discovery:**

When a device publishes to any MQTT topic, NeoMind:
1. Detects the new data source
2. Analyzes the payload structure
3. Creates a **draft device** in "Pending Devices" tab
4. User can approve, rename, and configure the device

---

### Method 2: HTTP Webhook

For devices that support HTTP but not MQTT.

**Webhook URL** (from `neomind system info`):
```
POST http://<SERVER_IP>:9375/api/devices/{device_id}/webhook
```

**Payload Format:**
```json
{
  "timestamp": 1716200000,
  "quality": 1.0,
  "data": {
    "temperature": 23.5,
    "humidity": 65
  }
}
```

**Steps:**
1. Create a device first: `neomind device create --name <NAME> --device-type <TYPE> --adapter-type <ADAPTER>`
2. Use the returned device ID in the webhook URL
3. Device sends HTTP POST with data payload

---

### Method 3: Manual Registration

For devices that need manual setup:

```bash
# Step 1: Check available device types
neomind device types list

# Step 2: Create device with specific type and adapter
neomind device create --name <NAME> --device-type <TYPE> --adapter-type <ADAPTER>

# adapter_type options:
#   mqtt        - MQTT protocol (default, bidirectional: telemetry + commands)
#   webhook     - HTTP webhook (receive-only: devices push data via POST)

# If no matching device type exists, create one first:
neomind device types create --name 'My Sensor' --metrics '[{"name":"temperature","display_name":"Temperature","data_type":"Float","unit":"°C"}]'

# Step 3: Verify device was created
neomind device get <DEVICE_ID>
```

---

## Step-by-Step Onboarding Guide

### Scenario A: ESP32/Arduino with MQTT

**User wants to connect an ESP32 temperature sensor.**

1. **Check infrastructure:**
```bash
neomind system info
```
→ Note the MQTT broker URL (e.g., `mqtt://192.168.1.100:1883` or `mqtts://...`), TLS status, auth status, and credentials.

2. **Give the user the connection info:**
   - Broker: from `mqtt.broker_address` (e.g., `192.168.1.100:1883`)
   - Protocol: from `mqtt.broker_url` scheme (`mqtt://` or `mqtts://`)
   - Topic: any (e.g., `sensors/esp32-01/temperature`)
   - Auth: if `auth_enabled` is true, provide credentials from `mqtt.credentials` array
   - TLS: if `tls_enabled` is true, inform user that device needs to trust the CA cert

3. **Example Arduino code (plain MQTT, no TLS):**
```cpp
#include <WiFi.h>
#include <PubSubClient.h>

const char* ssid = "YOUR_WIFI";
const char* password = "YOUR_PASSWORD";
const char* mqtt_server = "192.168.1.100";  // from neomind system info
const int mqtt_port = 1883;

WiFiClient espClient;
PubSubClient client(espClient);

void setup() {
  WiFi.begin(ssid, password);
  client.setServer(mqtt_server, mqtt_port);
  // If auth is enabled, use: client.connect("esp32-sensor-01", "username", "password");
  client.connect("esp32-sensor-01");
}

void loop() {
  float temp = readTemperature();  // your sensor reading
  char msg[32];
  snprintf(msg, 32, "{\"value\": %.1f}", temp);
  client.publish("sensors/esp32-01/temperature", msg);
  delay(5000);
}
```

4. **Example Python code (with TLS + auth):**
```python
import ssl
import paho.mqtt.client as mqtt
import json, time

client = mqtt.Client("python-sensor-01")

# If TLS is enabled
# client.tls_set(ca_certs="ca-cert.pem")  # Download from system info → CA cert
# client.tls_insecure_set(False)

# If auth is enabled
# client.username_pw_set("username", "password")

client.connect("192.168.1.100", 1883)

while True:
    data = {"temperature": 25.3, "humidity": 60.5}
    client.publish("sensors/python-01/data", json.dumps(data))
    time.sleep(10)
```

4. **After device sends data, it appears as a draft.** Help user find and approve it:
```bash
# Check pending device drafts
neomind device drafts list

# View draft details (sample data, detected metrics)
neomind device drafts get <DRAFT_ID>

# Approve and register the device
neomind device drafts approve <DRAFT_ID> --name "ESP32 Sensor" --type temp_sensor

# Or reject if unrecognized
neomind device drafts reject <DRAFT_ID>
```

**Auto-discovery configuration:**
```bash
# View current settings
neomind device drafts config

# Enable auto-approve (skip manual review)
neomind device drafts config --auto-approve true

# Disable auto-discovery
neomind device drafts config --enabled false
```

---

### Scenario B: Python Script with MQTT

**User wants to send data from a Python application.**

Use the same connection info from `neomind system info`. See the Python example in Scenario A above for TLS + auth configuration.

---

### Scenario C: HTTP Webhook Device

**User has a system that can only send HTTP requests.**

1. Create the device first:
```bash
neomind device create --name "Weather Station" --device-type weather-station --adapter-type webhook
```

2. Get the webhook URL for the device:
```bash
neomind device webhook-url <DEVICE_ID>
```

3. The webhook URL is: `POST http://<SERVER_IP>:9375/api/devices/{DEVICE_ID}/webhook`
```
Content-Type: application/json

{
  "data": {
    "temperature": 23.5,
    "humidity": 65
  }
}
```

3. User configures their system to POST to this URL.

---

### Scenario D: Existing MQTT Broker

**User already has an MQTT broker (e.g., EMQX, Mosquitto).**

1. NeoMind supports connecting to external brokers.
2. The external broker can be configured via the Web UI (Settings > MQTT).
3. Once configured, NeoMind subscribes to the external broker and auto-discovers devices.

---

## Common Questions & Answers

### "What's the MQTT broker address?"
Run `neomind system info` and check `mqtt.broker_address` and `mqtt.broker_url`. The URL includes the protocol scheme (`mqtt://` or `mqtts://`).

### "Do I need to install an MQTT broker?"
No. NeoMind includes an embedded MQTT broker. Devices can connect directly.

### "Does MQTT require authentication?"
Check `neomind system info` → `mqtt.auth_enabled`. If true, use the credentials from `mqtt.credentials` array. If false, anonymous connections are accepted.

### "Is TLS enabled?"
Check `neomind system info` → `mqtt.tls_enabled`. If true, use `mqtts://` scheme and ensure the device trusts the CA cert. If `tls_ca_available` is true, the CA cert can be downloaded from the web UI (Settings → MQTT Broker).

### "How do I know if my device is connected?"
```bash
neomind system info    # Check mqtt.connected and mqtt.devices_connected
neomind device list    # See all registered devices
neomind device get <ID>  # Check latest data from a device
```

### "My device sends data but it doesn't appear"
1. Check MQTT broker is connected: `neomind system info` → `mqtt.connected`
2. Check the device is publishing to the correct broker IP
3. Check network connectivity (same WiFi/LAN)
4. The device may appear in "Pending Devices" — check the Web UI

### "Can I use a custom MQTT topic?"
Yes! Any topic works. NeoMind auto-discovers data from any topic.

### "How do I send commands to a device?"
```bash
neomind device control <ID> <command> --params '<json>'
```
Commands are sent via MQTT to `{device_topic}/command` or `{device_topic}/downlink`.

### "What about Modbus/Serial/Zigbee/LoRa devices?"
These typically require a **gateway** that translates the protocol to MQTT or HTTP. The gateway sends data to NeoMind via MQTT or webhook.

---

## Quick Reference Card

| Item | Value |
|------|-------|
| MQTT Broker | `<SERVER_IP>:1883` (embedded, check `neomind system info` for actual) |
| MQTT Protocol | MQTT 3.1.1, `mqtt://` or `mqtts://` (check `tls_enabled`) |
| MQTT Auth | Check `auth_enabled` in system info, credentials in `credentials` array |
| Auto-Discovery | Enabled (`neomind/discovery/#`) |
| External Brokers | `neomind connector list` to see all |
| Webhook URL | `POST http://<SERVER_IP>:9375/api/devices/{device_id}/webhook` |
| API Base | `http://<SERVER_IP>:9375/api` |
| Check System | `neomind system info` |

## Device Management Commands

| Command | Description |
|---------|-------------|
| `neomind device list` | List all devices |
| `neomind device get <ID>` | Get device details |
| `neomind device create --name <NAME> [--device-type <T>] [--adapter-type <A>]` | Create device |
| `neomind device update <ID> [--name <N>] [--config '<JSON>']` | Update device |
| `neomind device delete <ID>` | Delete device |
| `neomind device get <ID>` | Get device details (metrics + commands) |
| `neomind device history <ID> [--metric <M>] [--time-range <R>]` | Telemetry history |
| `neomind device control <ID> <CMD> [--params '<JSON>']` | Send command |
| `neomind device types list` | List device types |
| `neomind device types create --name <N> --metrics '<JSON>'` | Create device type |
| `neomind device types get <ID>` | Get device type details |
| `neomind device webhook-url <ID>` | Get webhook push URL |
| `neomind device drafts list` | List pending device drafts |
| `neomind device drafts get <ID>` | View draft details and sample data |
| `neomind device drafts approve <ID> --name <N> --type <T>` | Approve and register draft |
| `neomind device drafts reject <ID>` | Reject and discard draft |
| `neomind device drafts config [--enabled] [--auto-approve] [--max-samples]` | View/configure auto-discovery |

## Connector Management

| Command | Description |
|---------|-------------|
| `neomind connector list` | List all MQTT connectors |
| `neomind connector create --name <N> --host <H> [--port <P>]` | Add external connector |
| `neomind connector get <ID>` | Get connector details |
| `neomind connector test <ID>` | Test connector connection |
| `neomind connector subscriptions` | List active subscriptions |

## Webhook Complete Flow

1. **Get webhook URL from system info:**
```bash
neomind system info
# Note the webhook URL: http://<SERVER_IP>:9375/api/devices/{device_id}/webhook
```

2. **Create the device first:**
```bash
neomind device create --name 'Weather Station' --adapter-type webhook
# Record the device ID from response
```

3. **Send data to webhook:**
```bash
curl -X POST http://<SERVER_IP>:9375/api/devices/<DEVICE_ID>/webhook \
  -H 'Content-Type: application/json' \
  -d '{"data": {"temperature": 23.5, "humidity": 65}}'
```

---

## Common Errors & Solutions

- **"MQTT broker not connected"**: The server may not be running. Check with `neomind system info`. If `mqtt.connected` is false, restart the server.
- **"Device not appearing after sending data"**: Verify the device is sending to the correct broker IP/port. Check network connectivity. Run `neomind device list` to see registered devices.
- **"Connection refused"**: Check if the server is running and the port (1883 for MQTT, 9375 for HTTP) is accessible. Firewall may be blocking the port.
- **"Auth failed / Not authorized"**: Auth is enabled on the broker. Run `neomind system info` to get valid credentials from `mqtt.credentials`, then configure the device with username/password.
- **"TLS handshake failed"**: TLS is enabled on the broker (port uses `mqtts://`). The device must be configured to use TLS and trust the CA certificate. The CA cert can be downloaded from the web UI.
- **"Received corrupt message"**: Device is connecting with plain TCP to a TLS-enabled port. Switch to `mqtts://` or configure TLS on the device side.
- **"Webhook returns 404"**: The device must be created first via `neomind device create` before sending webhook data. Use the exact device ID from the create response.
- **"Data not updating"**: Run `neomind device get <ID>` to check latest readings. If stale, verify the device is still publishing. Check `neomind system info` for MQTT connection status.
- **"Device sent data but not in device list"**: New devices appear as drafts. Run `neomind device drafts list` to find pending devices, then `neomind device drafts approve <ID>` to register.
- **"Too many unknown devices appearing"**: Adjust auto-discovery settings with `neomind device drafts config --max-samples 5` or disable with `--enabled false`.
