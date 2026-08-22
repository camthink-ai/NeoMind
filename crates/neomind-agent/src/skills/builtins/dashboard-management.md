---
id: dashboard-management
name: Dashboard Management & Component Creation
description: Use when the user wants to create or manage dashboards and their components — creating boards, adding/removing chart/widget/gauge components, binding data sources to widgets, sharing dashboards. Covers dashboard create/update/share/add-components even without saying 'dashboard' (e.g. '做个监控面板', '把温度画出来'). Includes 仪表盘/看板/组件/绑定数据.
category: dashboard
origin: builtin
priority: 85
token_budget: 14000
triggers:
  keywords: [dashboard, 仪表盘, 仪表板, create dashboard, 组件, widget, component, 数据源, data source, dashboard component, dashboard widget, 绑定数据, bind data, 创建仪表盘, 监控面板, share dashboard, 共享仪表盘, add-components, 添加组件, 删除组件, remove component]
  tool_target:
    - tool: dashboard
      actions: [list, get, create, update, delete, share, add-components, update-component, remove-components]
    - tool: widget
      actions: [list, get]
    - tool: device
      actions: [list, latest]
    - tool: extension
      actions: [info, list]
anti_triggers:
  keywords: [rule, 规则, agent, 代理, extension develop, 扩展开发]
---

# Dashboard Management & Component Creation

## Command Cheat-Sheet (run these via `shell`)

Always RUN the command yourself and report the real output.

| Command | Purpose |
|---|---|
| `neomind dashboard list` | List all dashboards |
| `neomind dashboard get <id>` | Dashboard details |
| `neomind dashboard create` | Create a new dashboard |
| `neomind dashboard update <id>` | Update a dashboard |
| `neomind dashboard add-components <id>` | Add components (append mode) |
| `neomind dashboard update-component <id> --component-id <cid> --set '<partial JSON>'` | **Tweak ONE component** (deep merge — preferred) |
| `neomind dashboard remove-components <id>` | Remove components by ID |
| `neomind dashboard delete <id>` | Delete a dashboard |
| `neomind dashboard share <id>` | Share a dashboard |

Creating dashboards with data-bound components is the most complex CLI operation. Follow the workflows below exactly.

## CRITICAL Rules

1. **NEVER guess metric names** — always discover them via `device list` (metric_fields per type) or `device get <ID>`
2. **Use `add-components` to add widgets** — this appends without replacing existing components
3. **To change ONE component's fields use `update-component`** — deep-merges a partial JSON into it; no need to re-send the whole component or remove+re-add
4. **`update --components` replaces ALL components** — avoid this unless intentionally replacing everything — plan layout accordingly
6. **NEVER use Python/pipe/file tricks** — each shell call is an isolated process; you cannot share data between calls via files, pipes, or variables. Build the complete JSON string inline.
7. **Use `widget get <type>` to inspect config_schema** before configuring unfamiliar widgets
8. **NEVER use emoji in component titles or descriptions** — use plain text labels only. Example: use "Temperature" not "Temperature", use "Humidity" not "Humidity"
9. **Charts (line-chart, area-chart, bar-chart, sparkline, pie-chart) MUST use `mode: "timeseries"`** — using `mode: "latest"` on charts causes rendering errors. Always include `timeWindow` (e.g., `"timeWindow": {"type": "last_24hours"}`) for proper historical data. Value-cards and indicators use `mode: "latest"`.

## Component Management Commands

| Command | Purpose |
|---------|---------|
| `dashboard add-components <ID> --components '<JSON>'` | **Append** new components (RECOMMENDED) |
| `dashboard update-component <ID> --component-id c1 --set '{"title":"New"}'` | **Change fields of ONE component** (deep merge — preferred for tweaks) |
| `dashboard remove-components <ID> --ids '["c1","c2"]'` | Remove components by ID |
| `dashboard update <ID> --name 'New Name'` | Update metadata only (name, description) |
| `dashboard update <ID> --components '<JSON>'` | Replace ALL components (dangerous!) |
| `dashboard delete <ID>` | Delete entire dashboard |

## Step-by-Step: Create a Data Dashboard

### Step 1: Discover Devices & Metrics

```bash
# List all devices
neomind device list

# Get real metric names for each device you want to display
neomind device get sensor-001
neomind device get sensor-002

# If using extension data, discover extension metrics
neomind extension get weather-forecast-v2
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

Use `add-components` — it appends without replacing existing components:

```bash
neomind dashboard add-components <DASHBOARD_ID> --components '[
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
]'
```

### Complete Example: Battery Dashboard

```bash
# Step 1: Discover metrics
neomind device list
neomind device get sensor-001
neomind device get sensor-002

# Step 2: Create dashboard
neomind dashboard create --name 'Battery Monitor'

# Step 3: Add ALL components using add-components (append mode)
neomind dashboard add-components <DASHBOARD_ID> --components '[
  {"id":"b1","type":"value-card","title":"Sensor 1 Battery","position":{"x":0,"y":0,"w":4,"h":2},
   "data_source":{"type":"telemetry","source":"device","id":"sensor-001","field":"battery","mode":"latest","sourceId":"sensor-001","metricId":"battery","timeRange":1,"limit":50},
   "display":{"unit":"%","format":".0f"}},
  {"id":"b2","type":"value-card","title":"Sensor 2 Battery","position":{"x":4,"y":0,"w":4,"h":2},
   "data_source":{"type":"telemetry","source":"device","id":"sensor-002","field":"battery","mode":"latest","sourceId":"sensor-002","metricId":"battery","timeRange":1,"limit":50},
   "display":{"unit":"%","format":".0f"}},
  {"id":"chart","type":"line-chart","title":"Battery Trends","position":{"x":0,"y":2,"w":12,"h":4},
   "data_source":[
     {"type":"telemetry","source":"device","id":"sensor-001","field":"battery","mode":"timeseries","sourceId":"sensor-001","metricId":"battery","timeRange":1,"limit":50,"timeWindow":{"type":"last_24hours"}},
     {"type":"telemetry","source":"device","id":"sensor-002","field":"battery","mode":"timeseries","sourceId":"sensor-002","metricId":"battery","timeRange":1,"limit":50,"timeWindow":{"type":"last_24hours"}}
   ],
   "display":{"unit":"%","showLegend":true}}
]'

# Step 4: Verify
neomind dashboard get <DASHBOARD_ID>
```

### Remove Components

```bash
# Remove specific components by their IDs
neomind dashboard remove-components <DASHBOARD_ID> --ids '["b1","chart"]'
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

# Retitle + resize + display tweak
neomind dashboard update-component <ID> --component-id c2 \
  --set '{"title":"Humidity","position":{"w":6},"display":{"unit":"%"}}'
```

Find component IDs with `neomind dashboard get <ID>` (each component has an `id`).

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

When adding new widgets to an existing dashboard, use `add-components`:

```bash
# Step 1: Check current layout to determine y position for new components
neomind dashboard get <ID>
# Note: find max(existing y + h) for the new y position

# Step 2: Add new components (they are appended, not replaced)
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
neomind extension get weather-forecast-v2

# Create dashboard with mixed sources
neomind dashboard create --name 'Weather Comparison'
neomind dashboard add-components <ID> --components '[
  {"id":"indoor","type":"value-card","title":"Indoor Temp",
   "position":{"x":0,"y":0,"w":4,"h":2},
   "data_source":{"type":"telemetry","source":"device","id":"sensor-001","field":"temperature","mode":"latest","sourceId":"sensor-001","metricId":"temperature","timeRange":1,"limit":50},
   "display":{"unit":"°C"}},
  {"id":"outdoor","type":"value-card","title":"Outdoor Temp",
   "position":{"x":4,"y":0,"w":4,"h":2},
   "data_source":{"type":"extension-metric","source":"extension","id":"weather-forecast-v2","field":"get_weather:temperature_c","mode":"timeseries","extensionId":"weather-forecast-v2","extensionMetric":"get_weather:temperature_c"},
   "display":{"unit":"°C"}},
  {"id":"compare","type":"line-chart","title":"Temperature Comparison",
   "position":{"x":0,"y":2,"w":12,"h":4},
   "data_source":[
     {"type":"telemetry","source":"device","id":"sensor-001","field":"temperature","mode":"timeseries","sourceId":"sensor-001","metricId":"temperature","timeRange":1,"limit":50,"timeWindow":{"type":"last_24hours"}},
     {"type":"extension-metric","source":"extension","id":"weather-forecast-v2","field":"get_weather:temperature_c","mode":"timeseries","extensionId":"weather-forecast-v2","extensionMetric":"get_weather:temperature_c","timeWindow":{"type":"last_24hours"}}
   ],
   "display":{"showLegend":true}}
]'
```

## Static Content (no data binding)

Markdown display widget for static content:
```bash
neomind dashboard add-components <ID> --components '[{
  "id":"md1","type":"markdown-display","title":"Report",
  "position":{"x":0,"y":0,"w":12,"h":4},
  "config":{"content":"# Status\nAll systems normal"}
}]'
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
| Components disappear after update | Used `update --components` which replaces all | Use `add-components` (add) or `update-component` (tweak) instead |
| "Device not found" | Wrong id | Run `neomind device list` for valid IDs |
| No data shows | Wrong field name | Run `neomind device list` (metric_fields) or `neomind device get <ID>` for exact metric names |
| Extension data not binding | Missing unified fields | Add `source:"extension"`, `id`, `field` (format: `COMMAND:FIELD`) |
| Chart shows no history | mode is "latest" | Charts need `mode:"timeseries"` with `timeWindow` |
| Position overlap | Same x,y coords | Each component needs unique position; grid is 12 columns |
| "Dashboard not found" | Wrong dashboard ID | Run `neomind dashboard list` for valid IDs |
| JSON parse error | Malformed JSON in --components | Validate JSON structure carefully |
