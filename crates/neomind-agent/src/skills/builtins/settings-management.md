---
id: settings-management
name: System Settings (Timezone & Data Retention)
category: settings
origin: builtin
priority: 75
token_budget: 5000
triggers:
  keywords: [settings, 设置, timezone, 时区, set-timezone, 设置时区, timezones, 时区列表, retention, 数据保留, 保留期, set-retention, cleanup, 清理, data cleanup, 数据清理, neomind settings]
  tool_target:
    - tool: shell
      actions: [timezone, set-timezone, timezones, retention, set-retention, cleanup]
anti_triggers:
  keywords: [device create, 创建设备, rule create, 创建规则, dashboard, 仪表盘, system info, broker 地址]
---

# System Settings: Timezone & Data Retention

`neomind settings` manages the global timezone and telemetry data retention.

## CRITICAL Rules

1. **Timezone + retention live under `neomind settings`, NOT `neomind system`.** `neomind system set-timezone` / `neomind system retention` do NOT exist — use `neomind settings set-timezone` / `neomind settings retention`.
2. **Timezone values are IANA format** — e.g. `Asia/Shanghai`, `America/New_York`, `Europe/London`. List valid ones with `neomind settings timezones`.
3. **Retention is a number of days** controlling how long telemetry data is kept.

## Command Reference

| Command | Purpose |
|---|---|
| `neomind settings timezone` | Get the current global timezone |
| `neomind settings set-timezone <IANA>` | Set the timezone, e.g. `neomind settings set-timezone Asia/Shanghai` |
| `neomind settings timezones` | List available timezones |
| `neomind settings retention` | Get data retention configuration |
| `neomind settings set-retention <days>` | Update retention (in days), e.g. `neomind settings set-retention 30` |
| `neomind settings cleanup` | Trigger a manual data cleanup now |

### Examples

```bash
neomind settings set-timezone Asia/Shanghai
neomind settings timezones
neomind settings retention
neomind settings set-retention 30
neomind settings cleanup
```
