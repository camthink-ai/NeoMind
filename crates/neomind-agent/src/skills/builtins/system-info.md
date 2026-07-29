---
id: system-info
name: System Information & Infrastructure
category: system
origin: builtin
priority: 88
token_budget: 4000
triggers:
  keywords: [system, 系统, system info, 系统信息, infrastructure, 基础设施, broker address, broker 地址, broker 端口, broker port, MQTT broker, mqtt 接入, 接入设备, 连接设备, connect device, how to connect, 如何连接, 如何接入, esp32, esp8266, webhook url, webhook 地址, network, 网络, 网络信息, 当前网络, server ip, 服务器 IP, 连接信息, connection info, tls, 认证, 端口, port, neomind system, 系统状态, 平台信息]
  tool_target:
    - tool: shell
      actions: [info]
anti_triggers:
  keywords: [device create, 创建设备, rule create, 创建规则, dashboard, 仪表盘, timezone, 时区, retention, 数据保留, connector create, 创建连接器]
---

# System Information & Infrastructure

`neomind system info` returns the NeoMind system/infrastructure details a user needs when connecting devices or asking for broker/network/server info.

## CRITICAL Rules

1. **RUN `neomind system info` yourself and report its actual output — do NOT just tell the user to run it, and do NOT narrate steps.** When the user wants the broker address / port / network info (e.g. to connect an ESP32/edge device via MQTT), the answer is one command: `neomind system info`. Execute it and summarize the result.
2. **`neomind system info` is the source of truth for the ACTIVE broker/network — NOT `neomind connector list`.** `connector list` shows *external* broker subscriptions the user configured; it does NOT return the built-in broker address/port/network the user needs to connect a device. For "what's the broker address / how do I connect" → `neomind system info`.
3. **Use `neomind system info` — NOT raw OS commands** (`ipconfig`, `ip a`, `ifconfig`, `hostname`). OS commands do NOT know the NeoMind broker/webhook config and may be blocked by the sandbox.
4. **`neomind system info` returns**: MQTT broker address + port + protocol (`mqtt://` or `mqtts://`), TLS status, auth status + credentials, webhook URL (for HTTP devices), network info (server IP, WiFi SSID), and device connection details (topics, payload formats).
5. **The only subcommand under `neomind system` is `info`.** Timezone and data retention are NOT here — they live under `neomind settings` (see the `settings-management` skill).

## Command Reference

### Show system info

```bash
neomind system info
```

Returns: MQTT broker (host/port/protocol), TLS status, auth status + credentials, webhook URL, network info (server IP, WiFi SSID), device connection topics/payload formats.

Use this whenever the user asks for: broker address, how to connect a device, server/network info, webhook URL, system status, or "what's the system setup".
