---
id: dashboard-management
summary: Create pages, add and bind visualization components.
not_for: [connecting devices (device-onboarding), alert conditions (rule-management)]
name: Dashboard Management & Component Creation
aliases: [dashboard, dashboards]
description: Use when the user wants to create or manage dashboards and their components — creating boards, adding/removing chart/widget/gauge components, binding data sources to widgets, sharing dashboards. Covers dashboard create/update/share/add-components even without saying 'dashboard' (e.g. '做个监控面板', '把温度画出来'). Includes 仪表盘/看板/组件/绑定数据.
category: dashboard
origin: builtin
priority: 85
token_budget: 14000
triggers:
  keywords: [dashboard, 仪表盘, 仪表板, create dashboard, 组件, widget, component, 数据源, data source, dashboard component, dashboard widget, 绑定数据, bind data, 创建仪表盘, 监控面板, share dashboard, 共享仪表盘, add-components, 添加组件, 删除组件, remove component, 显示到, 展示到, 放到首页, 上屏, show on, display on, home page, visualization, 可视化]
  tool_target:
    - tool: dashboard
      actions: [list, get, create, update, delete, share, add-components, update-component, remove-components]
    - tool: widget
      actions: [list, get]
    - tool: device
      actions: [list, get]
    - tool: extension
      actions: [get, list]
anti_triggers:
  keywords: [rule, 规则, agent, 代理, extension develop, 扩展开发]
---

# dashboard-management

Ask vs inspect vs mutate: how-to / layout questions → answer with zero writes; inspect with `dashboard list`/`get` and widget/device discovery; mutate only on an explicit create/update/add/remove request.
Always `dashboard get <ID>` before adding components — read occupied rows / next free row / existing ids, then `add-components` (append). Never guess layout.
Resize/tweak in place: keep current `position.x/y`; only patch `position.w`/`position.h` (grid units — "N行N列" means h=N,w=N). Do not ask for pixels. Prefer shell `params` object for `--set` JSON.
Inspect available dashboards and data sources. Discover component types/schema before adding unfamiliar widgets. Bind actual device, extension, transform or AI output fields.
Create or update the requested dashboard and widgets, then read them back. A saved dashboard does not prove every data binding has live values.
Preserve existing layout and unrelated widgets during edits. Use resource IDs, not display labels or guessed metric paths.
For transform outputs use transform ID and plain field. For extension sources use the format returned by component/data-source discovery.
Load widget/layout/binding reference sections before constructing complex configuration.

## Commands
| Command | Purpose |
|---|---|
| `neomind dashboard list` | List all dashboards |
| `neomind dashboard get <id>` | Dashboard details |
| `neomind dashboard create` | Create a new dashboard |
| `neomind dashboard update <id>` | Update name/description/layout (--components needs --replace-all) |
| `neomind dashboard add-components <id>` (with shell `params: [...]`) | Add components (append mode) |
| `neomind dashboard update-component <id> --component-id <cid>` (with shell `params: {...}`) | **Tweak ONE component** (deep merge — preferred) |
| `neomind dashboard remove-components <id>` (with shell `params: ["<cid>"]`) | Remove components by ID |
| `neomind dashboard delete <id>` | Delete a dashboard |
| `neomind dashboard share <id>` | Share a dashboard |

## Detailed reference
Use skill(action="load", id="dashboard-management", section="<exact returned title>") only for the needed detail. Section titles are returned with this overview.

<!-- SKILL_REFERENCE -->


# Dashboard Management & Component Creation

## Command Cheat-Sheet (via `shell` when mutating or inspecting live state)

Ask → answer from this guide with examples (**zero shell writes**). Inspect → read (`list`/`get`, widget/device discovery). Mutate → write only on an explicit create/update/add/remove request; then report the real output. Always `dashboard get <ID>` before adding components.

| Command | Purpose |
|---|---|
| `neomind dashboard list` | List all dashboards |
| `neomind dashboard get <id>` | Dashboard details |
| `neomind dashboard create` | Create a new dashboard |
| `neomind dashboard update <id>` | Update name/description/layout (--components needs --replace-all) |
| `neomind dashboard add-components <id>` (with shell `params: [...]`) | Add components (append mode) |
| `neomind dashboard update-component <id> --component-id <cid>` (with shell `params: {...}`) | **Tweak ONE component** (deep merge — preferred) |
| `neomind dashboard remove-components <id>` (with shell `params: ["<cid>"]`) | Remove components by ID |
| `neomind dashboard delete <id>` | Delete a dashboard |
| `neomind dashboard share <id>` | Share a dashboard |

Creating dashboards with data-bound components is the most complex CLI operation. Follow the workflows below exactly.

## CRITICAL Rules

1. **NEVER guess metric names** — always discover them via `device list` (metric_fields per type) or `device get <ID>`
2. **Use `add-components` to add widgets** — this appends without replacing existing components
3. **To change ONE component's fields use `update-component`** — deep-merges a partial JSON into it; no need to re-send the whole component or remove+re-add
4. **`update --components` is GATED** — it replaces ALL components and requires the explicit `--replace-all` flag; without it the command fails. You almost never want it: ADD → `add-components`, TWEAK → `update-component`
5. **ALWAYS `dashboard get <ID>` before adding components** — its output ends with a grid summary (occupied rows + `next free row: y=N`); place **new** widgets at that y. Component ids are listed too (avoid collisions). **Resize/move are different:** keep current x,y unless the user asks to move; only change `position.w`/`position.h`.
6. **Grid is 12 columns wide** — `w`/`h` are grid units (not pixels). "大概2行2列" → shell `params` `{"position":{"w":2,"h":2}}` at the existing x,y.
7. **Chat JSON = shell structured `params` ONLY** — preferred shape: `{ "command": "neomind dashboard add-components <ID>", "params": [ … ] }`. Never invent CLI `--params` for dashboard commands (primary flags are `--components` / `--set` / `--ids`). Do not paste huge `--components '[…]'` strings in Chat.
8. **Batch 1–2 components per call** — verify with `dashboard get`, then append the next row. Never first-turn a 5+ chart mega-array.
9. **NEVER put ephemeral `"data"` or `"label"` on value-card/charts** — those keys are rejected / not persisted. Live values require `data_source` (telemetry or extension-metric). Static prose → `markdown-display` + `config.content` only.
10. **Weather / extension metrics** — `extension invoke … refresh` or `get_weather` first (shell `params` object), then bind `data_source` with `type:"extension-metric"`, `field:"COMMAND:FIELD"` (e.g. `get_weather:temperature_c`).
11. **Use `widget get <type>` to inspect config_schema** before configuring unfamiliar widgets
12. **NEVER use emoji in component titles or descriptions** — plain text labels only ("Temperature" / "温度", not "🌡 Temperature")
13. **Charts MUST use `mode: "timeseries"`** with `timeWindow`. Value-cards use `mode: "latest"`.

## Minimal Component Shape (the schema validators REQUIRE exactly this)

```json
{"id": "my-unique-id", "type": "value-card", "title": "Temp", "position": {"x": 0, "y": 0, "w": 4, "h": 2}}
```

- `id` + `type` + `position` are REQUIRED; `title`, `data_source`, `display`, `config` are optional.
- `position` is an OBJECT with four required integers `x, y, w, h` — never a number or an array.
- `type` must be a real widget type (`widget list`); a typo'd type is rejected on add.
- CUSTOM widget (files under `frontend-components/`): register it FIRST — `neomind widget install <dir>` — then use the widget's ID as `type` (e.g. `"type": "drawing-component"`). `"custom"` is the manifest's category, NOT a valid type.
- Unsure about the shape? `dashboard get <ID>` prints existing components — copy one and change the fields (safest for unfamiliar widget types).

## Component Management Commands

| Command | Purpose |
|---------|---------|
| `shell`: `command="neomind dashboard add-components <ID>"`, `params=[{...}]` (terminal CLI: `--components '<JSON>'`) | **Append** new components — the reply lists added ids, confirms types verified, and gives the next free row (no follow-up get needed) |
| `shell`: `command="neomind dashboard update-component <ID> --component-id c1"`, `params={"title":"New"}` (terminal CLI: `--set '<JSON>'`) | **Change fields of ONE component** (deep merge — preferred for tweaks) |
| `shell`: `command="neomind dashboard remove-components <ID>"`, `params=["c1","c2"]` (terminal CLI: `--ids '<JSON>'`) | Remove components by ID |
| `neomind dashboard update <ID> --name 'New Name'` | Update metadata only (name, description) |
| `shell`: `command="neomind dashboard update <ID> --replace-all"`, `params=[...]` (terminal CLI: `--components '<JSON>'`) | Replace ALL components (gated — rarely needed) |
| `neomind dashboard delete <ID>` | Delete entire dashboard |

## Step-by-Step: Create a Data Dashboard

### Step 1: Discover Devices & Metrics

```bash
# List all devices
neomind device list

# Get real metric names for each device you want to display
neomind device get sensor-001
neomind device get sensor-002

# If using extension data, discover extension metrics
neomind extension get weather-forecast
```

**Record the exact metric names** — you will use them in data_source binding. NEVER guess metric names.

### Step 2: Check Available Widget Types

```bash
neomind widget list
# Returns all installed widget types with size constraints

neomind widget get value-card
# Returns config_schema describing accepted display and config fields
```

### Step 3: Create Dashboard

```bash
neomind dashboard create --name "Battery Monitor"
# Record the dashboard ID from the response
```

### Step 4: Add Components (RECOMMENDED)

Use shell structured `params` (1–2 cards per call). Appends without replacing existing components:

```json
{
  "command": "neomind dashboard add-components <DASHBOARD_ID>",
  "params": [
    {
      "id": "c1",
      "type": "value-card",
      "title": "Temperature",
      "position": {"x": 0, "y": 0, "w": 4, "h": 2},
      "data_source": {
        "type": "telemetry",
        "source": "device",
        "id": "sensor-001",
        "field": "temperature",
        "mode": "latest",
        "sourceId": "sensor-001",
        "metricId": "temperature",
        "timeRange": 1,
        "limit": 50
      },
      "display": {"unit": "°C", "format": ".1f"}
    }
  ]
}
```

Human CLI fallback only: `# Prefer shell params:[…] — string form is Human CLI fallback only
neomind dashboard add-components <ID> --components '[{...}]'` (never invent `--params`).

### Complete Example: Battery Dashboard

1. Discover: `neomind device list` / `device get sensor-001` / `device get sensor-002`
2. Create: `neomind dashboard create --name "Battery Monitor"`
3. Add **two value-cards** via shell `params` array (one call, ≤2 items):
```json
{
  "command": "neomind dashboard add-components <DASHBOARD_ID>",
  "params": [
    {"id":"b1","type":"value-card","title":"Sensor 1 Battery","position":{"x":0,"y":0,"w":4,"h":2},
     "data_source":{"type":"telemetry","source":"device","id":"sensor-001","field":"battery","mode":"latest","sourceId":"sensor-001","metricId":"battery","timeRange":1,"limit":50},
     "display":{"unit":"%","format":".0f"}},
    {"id":"b2","type":"value-card","title":"Sensor 2 Battery","position":{"x":4,"y":0,"w":4,"h":2},
     "data_source":{"type":"telemetry","source":"device","id":"sensor-002","field":"battery","mode":"latest","sourceId":"sensor-002","metricId":"battery","timeRange":1,"limit":50},
     "display":{"unit":"%","format":".0f"}}
  ]
}
```
4. Append the chart in a **second** call (still 1 item):
```json
{
  "command": "neomind dashboard add-components <DASHBOARD_ID>",
  "params": [
    {"id":"chart","type":"line-chart","title":"Battery Trends","position":{"x":0,"y":2,"w":12,"h":4},
     "data_source":[
       {"type":"telemetry","source":"device","id":"sensor-001","field":"battery","mode":"timeseries","sourceId":"sensor-001","metricId":"battery","timeRange":1,"limit":50,"timeWindow":{"type":"last_24hours"}},
       {"type":"telemetry","source":"device","id":"sensor-002","field":"battery","mode":"timeseries","sourceId":"sensor-002","metricId":"battery","timeRange":1,"limit":50,"timeWindow":{"type":"last_24hours"}}
     ],
     "display":{"unit":"%","showLegend":true}}
  ]
}
```
5. Verify: `neomind dashboard get <DASHBOARD_ID>`

### Remove Components

```json
{ "command": "neomind dashboard remove-components <DASHBOARD_ID>", "params": ["b1","chart"] }
```

### Delete Dashboard

```bash
neomind dashboard delete <DASHBOARD_ID>
```

## Modify ONE Component (PREFERRED for tweaks)

`update-component` deep-merges a partial JSON into an existing component — objects merge recursively, scalars/arrays replace. Only the fields you pass change; `id`/`type` are immutable. This beats remove+re-add or full `update --components`.

```bash
# Fix a chart's time window (only that field changes)
neomind dashboard update-component <ID> --component-id c3 \
  --set '{"data_source":{"timeWindow":{"type":"last_6hours"}}}'

# Re-bind to another device/metric
neomind dashboard update-component <ID> --component-id c1 \
  --set '{"data_source":{"id":"sensor-002","field":"humidity","metricId":"humidity"}}'

# Retitle + resize + display tweak (w/h = grid units; omit x/y to keep place)
neomind dashboard update-component <ID> --component-id c2 \
  --set '{"title":"Humidity","position":{"w":6},"display":{"unit":"%"}}'

# Preferred (no JSON quoting): shell tool `params` object
# command: neomind dashboard update-component <ID> --component-id c2
# params:  {"position":{"w":2,"h":2}}
```

### Resize example (Chat/shell tool — copy this shape)

```json
{
  "command": "neomind dashboard update-component <DASH_ID> --component-id <CID>",
  "params": { "position": { "w": 2, "h": 2 } }
}
```

`"N行N列"` / `2x2` means `h=N,w=N` grid cells. Keep current `x,y` unless the user asks to move.

Find component IDs with `neomind dashboard get <ID>` (each component has an `id`). Resize = patch `w`/`h` only; do not relocate to `next free row` unless adding a new widget.

## Expression Data Source (inline computed values — NO transform needed)

For computed values (averages, differences, sums of MULTIPLE devices), bind a
component directly with an inline expression instead of pre-creating a transform:

```json
{"type": "telemetry", "source": "expression", "mode": "latest",
 "expr": "avg(device:sensor-001:values.battery, device:sensor-002:values.battery)"}
```

- Refs: `device:<id>:<field>` (canonical) or `<id>.<field>` (sugar). Use REAL
  metric names from `device get` (e.g. `values.battery`).
- Operators: `+ - * / ( )`; functions: `avg sum min max abs round floor ceil sqrt pow`.
- Best for value-card / gauge / led-indicator (mode `latest`, live updates).
- Charts needing a COMPUTED CURVE (mode `timeseries`): community widgets support
  it; built-in charts still need a transform — create one via `transform create`.
- Example — 温差卡片:
  `--set '{"data_source":{"type":"telemetry","source":"expression","mode":"latest","expr":"device:sensor-001:temperature - device:sensor-002:temperature"}}'`

## DataSource Binding Reference

All data sources use **unified fields** (`source`/`mode`/`id`/`field`) plus legacy fields for backward compatibility.

### Unified Field System (v0.8.2+)

| source | mode | id | field | Component types |
|--------|------|----|-------|----------------|
| `device` | `latest` | device ID | metric name | value-card, led-indicator, gauge, progress-bar |
| `device` | `timeseries` | device ID | metric name | line-chart, area-chart, bar-chart, sparkline |
| `device` | `command` | device ID | command name | toggle-switch |
| `device` | `info` | device ID | property name | map-display |
| `extension` | `timeseries` | extension ID | `COMMAND:FIELD` | charts with extension data |
| `extension` | `command` | extension ID | command name | extension control buttons |
| `system` | `latest` | `neomind` | system metric | system stats display |

### Device Metrics

```json
{
  "type": "telemetry",
  "source": "device",
  "id": "<device-id>",
  "field": "<metric-name>",
  "mode": "latest",
  "sourceId": "<device-id>",
  "metricId": "<metric-name>",
  "timeRange": 1,
  "limit": 50
}
```

**IMPORTANT**:
- `type` MUST be `"telemetry"` (NOT `"device"`) — the frontend component editor uses `type` to determine the binding category. `type: "device"` is only for map markers (no metric).
- Must use real metric names from `device list` (metric_fields) or `device get <ID>`. Common names: `temperature`, `humidity`, `battery`, `cpu`, `memory`, `status`.
- Include both `field` (unified) and `metricId` (legacy) with the same value for full compatibility.

### Extension Metrics

```json
{
  "type": "extension-metric",
  "source": "extension",
  "id": "<ext-id>",
  "field": "<COMMAND>:<FIELD>",
  "mode": "timeseries",
  "extensionId": "<ext-id>",
  "extensionMetric": "<COMMAND>:<FIELD>"
}
```

**CRITICAL**:
- `id`/`field` = unified fields (always use these)
- `extensionId`/`extensionMetric` = legacy fields (include for backward compatibility)
- `field` MUST use `COMMAND:FIELD` format — e.g. `get_weather:temperature_c`, NOT bare `temperature_c`
- Discover via `extension get <ID>` → `commands[].id` + `commands[].output_fields[].name`

### Time Series (for charts)

Add `mode: "timeseries"` and time range to any data_source:
```json
{
  "type": "telemetry",
  "source": "device",
  "id": "sensor-01",
  "field": "temperature",
  "mode": "timeseries",
  "sourceId": "sensor-01",
  "metricId": "temperature",
  "timeRange": 1,
  "limit": 50,
  "timeWindow": {"type": "last_24hours"},
  "aggregateExt": "avg"
}
```

Time windows: `now`, `last_5min`, `last_15min`, `last_30min`, `last_1hour`, `last_6hours`, `last_24hours`, `today`, `this_week`
Aggregates: `raw`, `latest`, `avg`, `min`, `max`, `sum`, `count`, `delta`, `rate`
Charts accept `data_source` as **array** for multiple series.

### Common DataSource Errors

| Error | Wrong | Correct |
|-------|-------|---------|
| Device metric not binding in editor | `"type":"device"` | `"type":"telemetry"` — `device` type is only for map markers |
| Device metric missing `metricId` | Only `"field":"battery"` | Add `"metricId":"battery"` for editor compatibility |
| Extension data not binding | Missing `source:"extension"` | Add `"source":"extension"` and `"id"` |
| Extension field not binding | `"field":"temperature_c"` | `"field":"get_weather:temperature_c"` |
| Chart shows no history | `"mode":"latest"` | Charts need `"mode":"timeseries"` |
| No data shows | Guessed metric name | Run `device list` or `device get <ID>` first |
| "Device not found" | Wrong id | Run `device list` for valid IDs |
| Community widget shows "--" | Widget ignores `fetchData` | Widget must call `props.fetchData()` to get data |

## Widget Types Reference

| Category | Types | Min Size | Recommended |
|----------|-------|----------|-------------|
| Indicators | `value-card` | 3x2 | 3x2 or 4x2 |
| Indicators | `led-indicator` | 2x2 | 2x2 |
| Indicators | `sparkline` | 3x2 | 4x2 |
| Indicators | `progress-bar` | 3x1 | 4x1 |
| Charts | `line-chart` | 6x3 | 12x4 |
| Charts | `area-chart` | 6x3 | 12x4 |
| Charts | `bar-chart` | 6x3 | 12x4 |
| Charts | `pie-chart` | 4x4 | 4x4 |
| Charts | `radar-chart` | 4x4 | 6x4 |
| Controls | `toggle-switch` | 2x2 | 2x2 |
| Display | `markdown-display` | 12x2 | 12x4 |
| Display | `image-display` | 4x3 | 6x4 |
| Display | `image-history` | 6x4 | 12x4 |
| Display | `web-display` | 6x4 | 12x4 |
| Spatial | `map-display` | 6x4 | 12x6 |
| Spatial | `video-display` | 6x4 | 12x6 |
| Business | `agent-monitor-widget` | 6x4 | 12x6 |
| Business | `ai-analyst` | 12x4 | 12x6 |

## Layout Design Guide

### Grid System
- **12 columns wide**, unlimited rows
- Position: `{"x": 0, "y": 0, "w": 4, "h": 2}`
  - `x`: column offset (0-11)
  - `y`: row offset (0+)
  - `w`: width in columns
  - `h`: height in rows

### Alignment Patterns

| Layout | x positions | Example |
|--------|-----------|---------|
| 3 columns | 0, 4, 8 | Three value-cards side by side |
| 4 columns | 0, 3, 6, 9 | Four value-cards side by side |
| 2 columns | 0, 6 | Two charts side by side |
| Full width | 0, w=12 | Single chart spanning all columns |

### Layout Principles
1. **Same-type indicators on the same row** (value-cards at y=0)
2. **Charts get a full row** (w=12)
3. **New components start after existing ones** — y = max(existing y + h)
4. **No overlap** — each component needs unique x,y coordinates

### Layout Templates

**4 Indicators + 1 Chart:**
```
Row 0: [card1 x=0,w=3] [card2 x=3,w=3] [card3 x=6,w=3] [card4 x=9,w=3]
Row 2: [line-chart x=0,w=12,h=4]
```

**8 Indicators + 2 Charts:**
```
Row 0: [4 cards, x=0,3,6,9 w=3 each]
Row 2: [4 cards, x=0,3,6,9 w=3 each]
Row 4: [line-chart x=0,w=6,h=4] [bar-chart x=6,w=6,h=4]
```

**Mixed Device + Extension Dashboard:**
```
Row 0: [device-value x=0,w=4] [device-value x=4,w=4] [ext-value x=8,w=4]
Row 2: [chart with device+ext data_sources, x=0,w=12,h=4]
```

## Adding Components to Existing Dashboard

When adding new widgets to an existing dashboard, ALWAYS `dashboard get <ID>` first (layout + existing ids), then `add-components` (append — never full replace):

```bash
# Step 1: Check current layout to determine y position for new components
neomind dashboard get <ID>
# Note: find max(existing y + h) for the new y position

# Step 2: Add new components (they are appended, not replaced)
# Prefer shell params:[…] — string form is Human CLI fallback only
neomind dashboard add-components <ID> --components '[
  {"id":"new_chart","type":"line-chart","title":"New Chart",
   "position":{"x":0,"y":4,"w":12,"h":4},
   "data_source":{"type":"telemetry","source":"device","id":"sensor-001","field":"temperature","mode":"timeseries","sourceId":"sensor-001","metricId":"temperature","timeRange":1,"limit":50,"timeWindow":{"type":"last_24hours"}}}
]'
```

## Mixed Data Source Example (Device + Extension)

```bash
# Discover both device and extension metrics
neomind device get sensor-001
neomind extension get weather-forecast

# Create dashboard with mixed sources
neomind dashboard create --name 'Weather Comparison'
# Prefer shell params:[…] — string form is Human CLI fallback only
neomind dashboard add-components <ID> --components '[
  {"id":"indoor","type":"value-card","title":"Indoor Temp",
   "position":{"x":0,"y":0,"w":4,"h":2},
   "data_source":{"type":"telemetry","source":"device","id":"sensor-001","field":"temperature","mode":"latest","sourceId":"sensor-001","metricId":"temperature","timeRange":1,"limit":50},
   "display":{"unit":"°C"}},
  {"id":"outdoor","type":"value-card","title":"Outdoor Temp",
   "position":{"x":4,"y":0,"w":4,"h":2},
   "data_source":{"type":"extension-metric","source":"extension","id":"weather-forecast","field":"get_weather:temperature_c","mode":"timeseries","extensionId":"weather-forecast","extensionMetric":"get_weather:temperature_c"},
   "display":{"unit":"°C"}},
  {"id":"compare","type":"line-chart","title":"Temperature Comparison",
   "position":{"x":0,"y":2,"w":12,"h":4},
   "data_source":[
     {"type":"telemetry","source":"device","id":"sensor-001","field":"temperature","mode":"timeseries","sourceId":"sensor-001","metricId":"temperature","timeRange":1,"limit":50,"timeWindow":{"type":"last_24hours"}},
     {"type":"extension-metric","source":"extension","id":"weather-forecast","field":"get_weather:temperature_c","mode":"timeseries","extensionId":"weather-forecast","extensionMetric":"get_weather:temperature_c","timeWindow":{"type":"last_24hours"}}
   ],
   "display":{"showLegend":true}}
]'
```

## Static Content (no data binding)

**Only** `markdown-display` + `config.content` is for static text. Never put `"data"` / `"label"` on value-card.

```json
{
  "command": "neomind dashboard add-components <ID>",
  "params": [{
    "id":"md1","type":"markdown-display","title":"Report",
    "position":{"x":0,"y":0,"w":12,"h":4},
    "config":{"content":"# Status\nAll systems normal"}
  }]
}
```

## Share Dashboard

```bash
neomind dashboard share <ID> --public
neomind dashboard share <ID> --expires 3600
```

## Common Errors & Solutions

| Error | Cause | Solution |
|-------|-------|----------|
| "Invalid widget type" | Type doesn't exist | Run `neomind widget list` to see valid types |
| "--components needs --replace-all" error | Used `update --components` to add/tweak | ADD → `add-components`; TWEAK → `update-component`; only true full replacement passes `--replace-all` |
| "Device not found" | Wrong id | Run `neomind device list` for valid IDs |
| No data shows | Wrong field name | Run `neomind device list` (metric_fields) or `neomind device get <ID>` for exact metric names |
| Extension data not binding | Missing unified fields | Add `source:"extension"`, `id`, `field` (format: `COMMAND:FIELD`) |
| Chart shows no history | mode is "latest" | Charts need `mode:"timeseries"` with `timeWindow` |
| Position overlap | Same x,y coords | Each component needs unique position; grid is 12 columns |
| "Dashboard not found" | Wrong dashboard ID | Run `neomind dashboard list` for valid IDs |
| JSON parse error | Malformed JSON in --components / string path | Prefer shell `params: […]`; add 1–2 components; fix missing `}` |
| unexpected `--params` | Invented CLI `--params` on add-components | Use shell `params` array (maps to `--components`) — do not invent `--params` |
| unknown field `data`/`label` | Ephemeral static payload on value-card | Use `data_source` (or markdown-display `config.content` for static text) |
