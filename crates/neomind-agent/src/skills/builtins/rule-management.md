---
id: rule-management
summary: Create, modify, and diagnose condition-triggered automation rules.
not_for: [notification-channel setup (message-management), data conversion (transform-management)]
name: Rule Management Guide
aliases: [rule, rules]
description: Use when the user wants to create or manage rules — conditions (comparison/range/logical), triggers, actions (notify/execute/trigger-agent), thresholds, enabling/disabling. Covers rule create/update/enable/disable even without saying 'rule' (e.g. '温度超过30就告警', '低于20发通知'). Includes 规则/告警/条件/阈值/自动化.
category: rule
origin: builtin
priority: 85
token_budget: 5000
triggers:
  keywords: [rule, 规则, 创建规则, create rule, alert, 告警, 报警, trigger, 触发, automation, 自动化, condition, 条件, threshold, 阈值, 规则管理, rule create, rule update, rule enable, rule disable, 阈值判断, 超过, 低于, 大于, 小于, exceeds, below, above, 通知我, notify me]
  tool_target:
    - tool: rule
      actions: [list, get, create, update, delete, enable, disable, test, history]
anti_triggers:
  keywords: [dashboard, 仪表盘, agent, 代理, extension develop, 扩展开发, device connect, 设备连接]
---

# rule-management

Ask vs inspect vs mutate: how-to / "how would I…" questions → answer with zero writes; inspect with `get`/`list`/`explain`/`history`/`test`; mutate only on an explicit create/update/enable/disable/delete request.
Split intents: rename / notify text / frequency (`cooldown`/`for_duration`) are targeted updates; "why didn't it fire" is diagnosis (enabled → data → condition → cooldown/duration → channels) — do not rewrite the rule while diagnosing unless asked.
Use actual source IDs and metric names from current references or device/extension/transform inspection. Preserve the user's threshold and units; a current reading is not permission to choose a different threshold.
For a simple numeric threshold, use rule create flags; for logical/range/multi-action rules use --body JSON. Load the condition/action reference section before constructing an unfamiliar shape.
Source IDs use device:<id>:<field>, extension:<id>:<field>, or transform:<transform-id>:<field>. Transform ID is not output_prefix.
Actions: notify creates a message routed through configured channels; execute uses a discovered command and parameters; trigger_agent invokes an agent. Inspect channels only when delivery is requested or being diagnosed.
cooldown and for_duration are milliseconds. A manual rule test does not establish continuous-duration satisfaction or real delivery. Omitted trigger defaults to data_change; explicit trigger is an object with trigger_type.
Create/update, read back, then verify the requested behavior through explain/test/history as available. Do not inject live test telemetry or actuate a device solely to prove a rule without authorization.
When diagnosing, check enabled state, actual source values, missing/type-mismatched data, cooldown/duration, and action results. Missing values are unknown, not zero.

## Commands
| Command | Purpose |
|---|---|
| `neomind rule list` | List all rules |
| `neomind rule get <id>` | Rule details |
| `neomind rule create` | Create a new rule |
| `neomind rule update <id>` | Update a rule (rename/notify/cooldown, or re-target with `--trigger-device <dev> --metric <m> --operator <op> --threshold <v>`) |
| `neomind rule delete <id>` | Delete a rule |
| `neomind rule enable <id>` | Enable a rule |
| `neomind rule disable <id>` | Disable a rule |
| `neomind rule test <id>` | Test a rule |
| `neomind rule history <id>` | Rule execution history |

## Detailed reference
Use skill(action="load", id="rule-management", section="<exact returned title>") only for the needed detail. Section titles are returned with this overview.

<!-- SKILL_REFERENCE -->


# Rule Management

## Command Cheat-Sheet (via `shell` when mutating or inspecting live state)

Ask → answer from this guide with examples (**zero shell writes**). Inspect → read (`list`/`get`/`explain`/`history`/`test`). Mutate → write only on an explicit create/update/enable/disable/delete request; then report the real output.

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
| "how do I set frequency / cooldown / rename / change notify?" (ask only) | Answer from this guide with examples — **zero writes** until they ask to apply |
| "create / set up / add a rule" | Phase 1 → 2 → 3 → 4 below |
| "why didn't rule X fire / 排查 / 没触发" | Jump to **Diagnosis Flow** (inspect only; mutate only if asked) |
| "change freq / rename / notify text on rule X" | Inspect with `rule get`, then targeted `rule update` (see `rule update --help`) |
| "delete / disable / enable rule X" | **Quick Command Reference** |
| "rewrite / re-target an existing rule" | `rule get` current definition → confirm target/fields → apply with `rule update` options from `--help` (full **Diagnosis Flow** only for "why didn't fire") |

## Deployments involving rules, channels, and device control

When a request spans several resource types (for example, BMS ventilation rules, an alert channel, and immediate HVAC control), load the `message-management` and `device-onboarding` guides as well. Their CLI contracts differ from `rule create`:

- Discover the channel type and schema with `neomind message channel-types` and `neomind message channel-type-schema <type>` before `channel-create`. Use the documented `--type` and `--param key=value` or `--config` form; `--url` alone is not a channel-create option.
- Inspect the target device's supported metrics and commands with `neomind device get <id>` before rule creation or control. A light actuator may not expose an `occupancy` metric; source the condition from a device that actually reports occupancy.
- Send an immediate command as `neomind device control <id> <command> --param key=value` or `--params '{"key":"value"}'`; a bare value after the command is not an accepted parameter.
- Read each shell result's `exit_code`, stderr, and returned state. A completed tool call can contain a failed command. After errors, consult `--help` or the relevant guide and retry with a valid schema. Before saying a deployment is complete, verify the requested rule count, channel existence, and immediate device command result. Name any uncompleted item plainly.

## HARD GATE

```
NO RULE JSON CONSTRUCTION WITHOUT PRIOR DEVICE/METRIC DISCOVERY
```

Establish the source identity and schema before building the condition. Reuse current verified references; discover only missing or stale facts.

## Phase 1: Discovery — Completion Checklist

Check the required facts; do not repeat reads already established in this conversation.

- [ ] Ran `neomind device list` (or `neomind extension get <id>` for extension metrics)
- [ ] Identified the EXACT `device_id` (case-sensitive, hyphens matter) for each target
- [ ] Confirmed the EXACT metric name appears in `metric_fields` (NOT a guessed translation)
- [ ] If `metric_fields` is empty OR device offline OR >50 devices: ran `neomind device get <id>` per target
- [ ] Preserved the requested threshold and units; ask if a required threshold was not specified

**Output of Phase 1** (write it down before Phase 2):
```
device_id: <string>
metric_name: <string from metric_fields>
current_value: <number from device get>
operator + threshold: <requested by user>
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
- `transform:<transform_id>:<field>` — needs `neomind transform list` first. **`transform_id` is the transform's `id` from `transform list` (NOT its `output_prefix`). `field` is the plain output field name (e.g. `roi_count`, `full_dustbin_count`) — NOT a dotted `extension.field`, and NOT the `output_prefix`.**

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
- [ ] threshold and units match the user request
- [ ] If external notification delivery is requested: inspect the intended channel and its routing filters. Creating a platform message does not require configuring a new external channel.
- [ ] If action is `execute`: target device exists in `neomind device list` and has the command listed under `command_fields`
- [ ] `cooldown` is set explicitly when `notify` is the action

## Phase 4: Activate & Verify

```bash
# FAST PATH for single-metric threshold rules — flags, no JSON.
# Rule is ENABLED by default on creation; cooldown defaults to 300000 (5 min).
neomind rule create --name "High Temp" \
  --trigger-device living-room-sensor --metric temperature \
  --operator greater_than --threshold 30 \
  --notify "Too hot: {value}"

# FULL FORM for anything else (range/logical/multi-action/schedule):
Chat/shell preferred: `{ "command": "neomind rule create", "params": {…rule object…} }` (maps to `--body`). Human CLI: `neomind rule create --body '<your_json>'`
# Note the returned rule ID.

# With authorized test input: evaluate the condition; this alone does not prove duration or physical delivery.
neomind rule test <ID> --input '{"<metric>": <value_above_threshold>}'

# For ongoing monitoring of when/how it fires.
neomind rule history <ID>
```

Fast-path variants: `--operator` accepts greater_than | less_than | greater_equal | less_equal | equal | not_equal; `--severity` (info | warning | critical | emergency, default warning) and `--cooldown <ms>` tune the notify action; non-device metrics use `--source extension:<id>:<metric>` (or `transform:<id>:<field>`) instead of `--trigger-device`/`--metric`.

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
| Skipping `rule test` after creation | "Works on paper" but real flow broken | Use authorized test input; distinguish simulation from live delivery |

## Quick Command Reference

```bash
neomind rule list                         # all rules
neomind rule get <ID>                     # inspect one
neomind rule create --name <N> --trigger-device <D> --metric <M> --operator <OP> --threshold <V> --notify <MSG>
                                          # fast path: single-metric threshold rule (flags)
neomind rule create --body '<JSON>'       # full form: range/logical/multi-action (enabled by default)
neomind rule update <ID> --body '<JSON>'  # modify fields (ID also accepted as --id <ID>)
neomind rule update <ID> --trigger-device sensor-001 --metric values.battery --operator less_than --threshold 15  # re-target / re-threshold (actions preserved)
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
| "Invalid DataSourceId" | Wrong source format | Use `device:ID:METRIC` / `extension:ID:METRIC` / `transform:ID:FIELD` |
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
