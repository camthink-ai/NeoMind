---
id: rule-management
name: Rule Management Guide
category: rule
origin: builtin
priority: 85
token_budget: 5000
triggers:
  keywords: [rule, 规则, 创建规则, create rule, alert, 告警, 报警, trigger, 触发, automation, 自动化, condition, 条件, action, 动作, threshold, 阈值, notification, 通知, 规则管理, rule create, rule update, rule enable, rule disable, 阈值判断, 超过, 低于, 大于, 小于, temperature, 温度, humidity, 湿度, battery, 电池]
  tool_target:
    - tool: rule
      actions: [list, get, create, update, delete, enable, disable, test, history]
anti_triggers:
  keywords: [dashboard, 仪表盘, agent, 代理, extension develop, 扩展开发, device connect, 设备连接]
---

# Rule Management

## Command Cheat-Sheet (run these via `shell`)

Always RUN the command yourself and report the real output.

| Command | Purpose |
|---|---|
| `neomind rule list` | List all rules |
| `neomind rule get <id>` | Rule details |
| `neomind rule create` | Create a new rule |
| `neomind rule update <id>` | Update a rule |
| `neomind rule delete <id>` | Delete a rule |
| `neomind rule enable <id>` | Enable a rule |
| `neomind rule disable <id>` | Disable a rule |
| `neomind rule test <id>` | Test a rule |
| `neomind rule history <id>` | Rule execution history |

Create, update, diagnose, or delete event-driven rules over device / extension / transform metrics. Skipped discovery is the #1 cause of silent rule failures.

## When to Load This Skill

| User intent | Section to follow |
|---|---|
| "create / set up / add a rule" | Phase 1 → 2 → 3 → 4 below |
| "why didn't rule X fire / 排查 / 没触发" | Jump to **Diagnosis Flow** |
| "delete / disable / enable rule X" | **Quick Command Reference** |
| "update an existing rule" | Run diagnosis first, then **Phase 2** with new JSON |

## HARD GATE

```
NO RULE JSON CONSTRUCTION WITHOUT PRIOR DEVICE/METRIC DISCOVERY
```

**You MUST complete Phase 1 before Phase 2.** Violating this is the single most common failure mode: rules silently never fire because the device_id or metric name was guessed.

## Phase 1: Discovery — Completion Checklist

Every item must be ticked. Re-run if any is unchecked.

- [ ] Ran `neomind device list` (or `neomind extension get <id>` for extension metrics)
- [ ] Identified the EXACT `device_id` (case-sensitive, hyphens matter) for each target
- [ ] Confirmed the EXACT metric name appears in `metric_fields` (NOT a guessed translation)
- [ ] If `metric_fields` is empty OR device offline OR >50 devices: ran `neomind device get <id>` per target
- [ ] Noted current value range (to pick a sensible threshold, not a magic number)

**Output of Phase 1** (write it down before Phase 2):
```
device_id: <string>
metric_name: <string from metric_fields>
current_value: <number from device get>
operator + threshold: <chosen based on current_value>
```

## Phase 2: Construct Rule JSON

Minimal canonical shape (only `name`, `condition.source`, `condition.operator`, `condition.threshold`, `actions` are required):

```json
{
  "name": "High Temperature Alert",
  "condition": {
    "condition_type": "comparison",
    "source": "device:living-room-sensor:temperature",
    "operator": "greater_than",
    "threshold": 30
  },
  "cooldown": 300000,
  "actions": [{"type": "notify", "message": "Temp: {value}°C", "severity": "warning"}]
}
```

### Condition Types — Pick One via Decision Tree

```
What does the user want?
├── One metric vs a number
│   → comparison  (operators: greater_than | less_than | greater_equal | less_equal | equal | not_equal)
├── One metric in a band (min..max inclusive)
│   → range        { "min": 18, "max": 28 }
├── Multiple metrics all must match
│   → logical AND  { "conditions": [...] }
├── Multiple devices, same metric, any triggers
│   → logical OR   { "conditions": [...] }
├── Negate any sub-condition
│   → logical NOT  { "conditions": [<single>] }
└── String metric (text matching)
    → string ops   (contains | starts_with | ends_with | regex), threshold is a string
```

### Source Format (3 forms only)

- `device:<device_id>:<metric>` — most common
- `extension:<ext_id>:<metric>` — needs `neomind extension get <id>` first
- `transform:<output_prefix>:<field>` — needs `neomind transform list` first

### Action Types

| Type | When to use | Key fields |
|---|---|---|
| `notify` | Send alert to all configured channels | `message` (supports `{value}`, `{source_id}`), `severity` (info\|warning\|critical\|emergency) |
| `execute` | Trigger a device/extension command | `target`, `target_type`, `command`, `params` |
| `trigger_agent` | Hand off to AI for complex response | `agent_id`, `input` |

### Optional Tuning Fields

- `for_duration` (ms): condition must hold this long before firing. Use 30000–60000 for noisy signals to avoid flapping. Default 0 = fire instantly.
- `cooldown` (ms): minimum gap between triggers. Default 60000. **Always set explicitly for `notify` actions** — alert storms happen without it.
- `trigger` (OPTIONAL — defaults to `data_change` when omitted, both via API and CLI): an **internally-tagged enum** (`trigger_type` is the tag, INSIDE the `trigger` object). Three shapes only:
  - Omit entirely → `{"trigger_type":"data_change"}` (sources auto-extracted from condition). This is what you want for any threshold/band/logic rule.
  - Cron schedule: `"trigger": {"trigger_type": "schedule", "cron": "0 */5 * * * *"}`
  - Manual only: `"trigger": {"trigger_type": "manual"}`
  - ❌ NEVER use: `"trigger": "data_change"` (bare string), `"trigger": {"data_change": {}}` (externally tagged), or `"trigger_type": "data_change"` (flat, at root). All three are rejected by the API. The tag field `trigger_type` MUST live INSIDE the `trigger` object.

## Phase 3: Validate — Pre-Flight Checklist

Before running `neomind rule create`, tick every box:

- [ ] `device_id` in JSON matches Phase 1 output character-for-character
- [ ] metric name in JSON matches Phase 1 output character-for-character
- [ ] operator matches metric data type (numeric vs string)
- [ ] threshold is realistic vs `current_value` from Phase 1 (not magic number)
- [ ] If action is `notify`: at least one message channel exists — run `neomind message channel-list`. If empty, load **message-management** skill first.
- [ ] If action is `execute`: target device exists in `neomind device list` and has the command listed under `command_fields`
- [ ] `cooldown` is set explicitly when `notify` is the action

## Phase 4: Activate & Verify

```bash
# Rule is ENABLED by default on creation — no separate enable step needed.
neomind rule create --body '<your_json>'
# Note the returned rule ID.

# For threshold rules: inject a test value to confirm it fires end-to-end.
neomind rule test <ID> --input '{"<metric>": <value_above_threshold>}'

# For ongoing monitoring of when/how it fires.
neomind rule history <ID>
```

## Diagnosis Flow — "Why didn't my rule fire?"

Run these in order. Stop at the first failure.

1. **Rule enabled?** `neomind rule get <ID>` → check `enabled: true`. If false: `neomind rule enable <ID>`.
2. **Device actually online?** `neomind device list` → device status. If offline, rule has no data to evaluate.
3. **Data fresh?** `neomind device get <ID>` → last metric value. If stale (older than `offline_timeout`), no recent evaluations.
4. **Condition match at least once?** `neomind rule history <ID>` → look for `evaluated: true, matched: false`. If `matched: true` but no notification → channel problem (jump to message-management).
5. **Cooldown blocking?** History shows `evaluated: true, matched: true` but `skipped: cooldown`. Raise `cooldown` only if false-positives are flooding; otherwise wait it out.
6. **`for_duration` not yet satisfied?** Condition must hold continuously for `for_duration` ms. History shows recent `matched: true` but no fire → wait longer.
7. **Source ID correct?** Verify `source` string in the rule JSON exactly matches `device:<id>:<metric>` from `device get`. Typos are silent.

## Red Flags — Stop If You See These

| Sign | Why it's wrong | Fix |
|---|---|---|
| Constructing JSON before running `device list` | Guessed IDs → rule never fires | Back to Phase 1 |
| Threshold pulled from user request verbatim with no sanity check | Often wrong unit/range | Cross-check vs `current_value` |
| `notify` action but haven't run `channel-list` | Alerts vanish silently | Verify ≥1 channel, or load message-management |
| `cooldown: 0` or unset on `notify` rule | Alert storms | Set ≥300000 (5 min) |
| String operator (`contains`) on numeric metric | Always errors or never matches | Use numeric operator |
| Skipping `rule test` after creation | "Works on paper" but real flow broken | Always run with synthetic input |

## Quick Command Reference

```bash
neomind rule list                         # all rules
neomind rule get <ID>                     # inspect one
neomind rule create --body '<JSON>'       # create (enabled by default)
neomind rule update <ID> --body '<JSON>'  # modify fields
neomind rule enable <ID>                  # re-enable a paused rule
neomind rule disable <ID>                 # pause without deleting
neomind rule delete <ID>                  # permanent removal
neomind rule test <ID> --input '<JSON>'   # inject synthetic metric
neomind rule history <ID>                 # evaluation log
```

## Common Errors & Solutions

| Error | Cause | Solution |
|-------|-------|----------|
| "Missing 'name' field" | JSON body missing name | Add `"name": "..."` |
| "Invalid DataSourceId" | Wrong source format | Use `device:ID:METRIC` / `extension:ID:METRIC` / `transform:PREFIX:FIELD` |
| "Device not found in condition" | Wrong device ID (guessed) | Run Phase 1 discovery |
| "Unknown metric" | Wrong metric name (guessed) | Run `device list` (check `metric_fields`) or `device get <ID>` |
| Rule not triggering | Disabled | `neomind rule enable <ID>` |
| Rule triggers too often | No cooldown / threshold too tight | Add `"cooldown": 300000` |
| Rule matches but no notification | No channels configured | Load **message-management** skill |
| Invalid JSON | Quotes/brackets/commas | Validate JSON syntax |

## Related Skills

- **message-management** — create Telegram / email / webhook channels for `notify` actions. **Load before completing Phase 3 if channels list is empty.**
- **transform-management** — pre-process metrics (rolling avg, unit conversion) before rule condition
- **agent-management** — when using `trigger_agent` action; agent must exist first
- **device-onboarding** — if `device list` is empty or target device missing, the device isn't registered yet
