---
id: widget-development
name: Custom Widget Development
description: Use when the user wants to DEVELOP a custom dashboard widget — creating a component bundle (IIFE/JSX/manifest), custom charts/gauges. Distinct from INSTALLING widgets (that's widget-management). Covers widget create/install even without saying 'widget' (e.g. '写个自定义图表组件'). Includes 组件开发/自定义图表/widget 开发.
category: widget
origin: builtin
priority: 85
token_budget: 14000
triggers:
  keywords: [widget, custom widget, 自定义组件, widget create, 组件开发, component, IIFE, bundle.js, manifest, 仪表盘组件, dashboard widget, 自定义图表, custom chart, jsxRuntime, device binding]
  tool_target:
    - tool: widget
      actions: [create, install, get, list, uninstall, market-list, market-install]
anti_triggers:
  keywords: [extension develop, 扩展开发, rule, 规则, device connect, 设备连接, agent, 代理]
---

# Custom Widget Development

Custom widgets are React components for NeoMind dashboards. No build tools — just `manifest.json` + `bundle.js`.

## CRITICAL: Widget Development Workflow

```bash
# Step 1: Scaffold
neomind widget create "My Widget" --widget-type chart
# Creates: data/frontend-components/<widget-id>/manifest.json + bundle.js

# Step 2: Edit both files — use file_write or file_edit tools (preferred over shell commands)
```

**Editing files (use file_write / file_edit instead of shell):**
```
# Write complete manifest.json:
file_write(path="frontend-components/<widget-id>/manifest.json", content='{"id":"my-widget",...}')

# Write complete bundle.js:
file_write(path="frontend-components/<widget-id>/bundle.js", content='var MyWidget = (function() {...})();')

# Edit specific part of bundle.js:
file_edit(path="frontend-components/<widget-id>/bundle.js", old_string="Hello", new_string="World")
```

```bash
# Step 3: Install (re-run after each edit to apply changes)
neomind widget install data/frontend-components/<widget-id>

# Step 4: Use in dashboard
neomind dashboard add-components <DASHBOARD_ID> --components '[{
  "id": "my-widget-1",
  "type": "custom",
  "title": "My Widget",
  "position": {"x": 0, "y": 0, "w": 6, "h": 4},
  "data_source": {"type": "telemetry","source":"device","id":"DEVICE_ID","field":"temperature","mode":"latest","sourceId":"DEVICE_ID","metricId":"temperature","timeRange":1,"limit":50}
}]'
```

## IIFE Bundle Format (REQUIRED)

NeoMind provides `window.React`, `window.jsxRuntime.jsx`, and `window.jsxRuntime.jsxs`.

**Two valid formats:**

### Format A: Variable assignment (preferred — used by real components)
```javascript
var MyWidget = (function() {
  var React = window.React;
  var jsx = window.jsxRuntime.jsx;
  var jsxs = window.jsxRuntime.jsxs;

  function MyWidget(props) {
    var config = props.config || {};
    return jsx('div', {
      className: 'flex flex-col items-center justify-center h-full w-full p-3',
      children: jsx('span', { className: 'text-2xl font-bold text-foreground', children: 'Hello' })
    });
  }

  return { default: MyWidget, MyWidget: MyWidget };
})();
```
manifest.json: `"global_name": "MyWidget"` — the IIFE assigns to `var MyWidget` which becomes `window.MyWidget`.

### Format B: Global assignment (used by scaffold templates)
```javascript
(function(global) {
  'use strict';
  var React = global.React;
  function MyWidget(props) {
    return React.createElement('div', {style:{width:'100%',height:'100%'}}, 'Hello');
  }
  global['MyWidget'] = MyWidget;
})(window);
```

**Rules:**
- `window.React` provides React hooks (useState, useEffect, useRef, etc.)
- `window.jsxRuntime.jsx/jsxs` for JSX-like syntax (cleaner than createElement)
- NO JSX syntax — use `jsx('div', {className: '...', children: ...})`
- NO import/require — all globals from window
- Use Tailwind utility classes (preferred) or CSS variables for styling
- Bundle must be under 50KB
- Container must fill space: `className: 'h-full w-full'` or `style: {width:'100%',height:'100%'}`
- Every element in `jsxs` children arrays must have a unique `key` prop
- **CRITICAL: `props.dataSource` is the raw config object (NOT resolved data). You MUST call `props.fetchData()` to get actual values.** Never read `props.dataSource.value` — it does not exist. Use `useState` + `useEffect` to call `fetchData()` on mount.

## React Hooks Available

```javascript
var React = window.React;

// State
var stateArr = React.useState(initialValue);
var value = stateArr[0], setValue = stateArr[1];

// Effect with cleanup
React.useEffect(function() {
  var timer = setInterval(function() { /* ... */ }, 1000);
  return function() { clearInterval(timer); };  // cleanup
}, []);

// Ref (for canvas, DOM access)
var ref = React.useRef(null);
// Usage: jsx('canvas', { ref: ref, ... })
```

## Props Interface

| Prop | Type | Description |
|------|------|-------------|
| `props.dataSource` | object | Live data: `.value`, `.timeSeries`, `.isLoading`, `.unit`, `.min`, `.max` |
| `props.fetchData` | function | Async data fetch: `fetchData(options?)` → `{ value }` or `{ series }` |
| `props.config` | object | User config from config_schema (always default with `|| {}`) |
| `props.title` | string | Widget title |
| `props.id` | string | Instance ID |
| `props.editMode` | boolean | Dashboard edit mode |
| `props.deviceContext` | object | Bound device info (if has_device_binding) |
| `props.sendDeviceCommand` | function | Send command to bound device |

### props.dataSource Structure
```javascript
// Single value (for gauge, stat)
var value = props.dataSource && props.dataSource.value;   // number or string
var unit = props.dataSource && props.dataSource.unit;      // e.g., "°C"

// Time-series (for charts)
var points = (props.dataSource && props.dataSource.timeSeries) || [];
// Each point: { timestamp: 1716550000000, value: 23.5 }

// Loading state
var isLoading = props.dataSource && props.dataSource.isLoading;
```

### props.deviceContext Structure (device-bound widgets)
```javascript
var device = props.deviceContext && props.deviceContext.device;
var deviceType = props.deviceContext && props.deviceContext.deviceType;
var sendCmd = props.sendDeviceCommand;

if (!device) return jsx(NoDevice, {});  // always handle no-device case

var vals = device.currentValues || {};        // { "temperature": 23.5, "humidity": 60 }
var online = device.status === 'online';
var metrics = (deviceType && deviceType.metrics) || [];  // metric definitions
var commands = (deviceType && deviceType.commands) || []; // available commands

// Send command (async)
sendCmd('command_name').then(function() { /* success */ }).catch(function() { /* error */ });
```

### props.fetchData (unified data fetch)

All community widgets receive `fetchData` for on-demand data access without React hooks:

```javascript
// Fetch configured DataSource data
var result = await props.fetchData();
// result → { value: <any> } for latest/info modes
// result → { series: [{ timestamp, value }] } for timeseries mode
// result → null if no dataSource configured

// With time range options
var result = await props.fetchData({ timeRange: 24, limit: 200 });
```

**Return format by mode:**

| Mode | Returns | Use Case |
|------|---------|----------|
| `latest` | `{ value: <any> }` | Single metric value |
| `timeseries` | `{ series: [{ timestamp, value }] }` | Historical chart data |
| `info` | `{ value: <string> }` | Device metadata |
| `command` | `{ value: undefined }` | Command sources |

## React Key Rules

When using `jsxs` with array children, **every element must have a `key` prop**:

```javascript
// CORRECT — use jsxs + keys for arrays
jsxs('div', { children: [
  jsx('span', { key: 'label', children: 'Status' }),
  jsx('span', { key: 'value', children: 'Online' })
]})

// WRONG — missing keys causes React warning
jsxs('div', { children: [
  jsx('span', { children: 'Status' }),
  jsx('span', { children: 'Online' })
]})
```

**Rules:**
- Use `jsxs` (not `jsx`) when children is an array of 2+ elements
- Every element in a `jsxs` children array needs a unique `key`
- For `.map()` results, use a unique property: `key: item.id` or `key: item.name`
- Single-child renders can use `jsx` without keys

## Styling: Tailwind Classes & CSS Variables

**Always use Tailwind classes or CSS design tokens — NEVER hardcoded colors.**

### Tailwind utility classes (preferred):
```
Layout: flex, flex-col, items-center, justify-center, gap-1, p-3, w-full, h-full
Text: text-foreground, text-muted-foreground, text-2xl, font-bold, font-mono
Background: bg-muted, bg-muted-30, bg-card, bg-success, bg-warning, bg-error
Border: border, border-border, rounded-lg, rounded-md
Status: text-success, text-warning, text-error, bg-success, bg-error
Numeric: tabular-nums (for aligned numbers)
```

### CSS Variables (for inline styles):
```
var(--color-text-primary), var(--color-text-muted)
var(--color-success), var(--color-warning), var(--color-error)
var(--color-border), var(--chart-1) through var(--chart-6)
```

## Complete Widget Templates

### Template 1: Value Card (single metric display)

```javascript
var ValueCardWidget = (function() {
  var React = window.React;
  var jsx = window.jsxRuntime.jsx;
  var jsxs = window.jsxRuntime.jsxs;

  function ValueCard(props) {
    var config = props.config || {};
    var label = config.label || props.title || '';
    var ds = props.dataSource || {};
    var value = ds.value != null ? ds.value : '-';
    var unit = ds.unit || '';
    var isLoading = ds.isLoading;

    if (isLoading) {
      return jsx('div', {
        className: 'flex items-center justify-center h-full w-full',
        children: jsx('span', { className: 'text-muted-foreground', children: 'Loading...' })
      });
    }

    return jsxs('div', {
      className: 'flex flex-col items-center justify-center h-full w-full p-3',
      children: [
        jsx('div', {
          key: 'val',
          className: 'text-3xl font-bold font-mono tabular-nums text-foreground',
          children: String(value) + unit
        }),
        jsx('div', {
          key: 'label',
          className: 'text-xs text-muted-foreground mt-1',
          children: label
        })
      ]
    });
  }

  return { default: ValueCard, ValueCard: ValueCard };
})();
```

### Template 2: Clock (self-updating, no dataSource)

```javascript
var ClockWidget = (function() {
  var React = window.React;
  var jsx = window.jsxRuntime.jsx;
  var jsxs = window.jsxRuntime.jsxs;

  function Clock(props) {
    var timeState = React.useState(new Date());
    var time = timeState[0];
    var setTime = timeState[1];

    React.useEffect(function() {
      var timer = setInterval(function() { setTime(new Date()); }, 1000);
      return function() { clearInterval(timer); };
    }, []);

    var config = props.config || {};
    var showSeconds = config.showSeconds !== false;
    var h = String(time.getHours()).padStart(2, '0');
    var m = String(time.getMinutes()).padStart(2, '0');
    var s = String(time.getSeconds()).padStart(2, '0');

    return jsxs('div', {
      className: 'flex flex-col items-center justify-center h-full w-full p-3 select-none border border-border rounded-lg',
      children: [
        jsxs('div', {
          key: 'time',
          className: 'flex items-baseline gap-0.5',
          children: [
            jsx('span', { className: 'text-3xl font-mono font-bold tracking-tight text-foreground tabular-nums', children: h + ':' + m }),
            showSeconds ? jsx('span', { className: 'text-lg font-mono text-muted-foreground tabular-nums', children: ':' + s }) : null
          ]
        })
      ]
    });
  }

  return { default: Clock, Clock: Clock };
})();
```

### Template 3: Gauge (progress bar with color thresholds)

```javascript
var GaugeWidget = (function() {
  var React = window.React;
  var jsx = window.jsxRuntime.jsx;
  var jsxs = window.jsxRuntime.jsxs;

  function Gauge(props) {
    var config = props.config || {};
    var ds = props.dataSource || {};
    var value = ds.value || 0;
    var max = config.max || 100;
    var min = config.min || 0;
    var unit = config.unit || '';
    var pct = Math.min(Math.max((value - min) / (max - min || 1) * 100, 0), 100);

    var barClass = pct > 75 ? 'bg-error' : pct > 50 ? 'bg-warning' : 'bg-success';

    return jsxs('div', {
      className: 'flex flex-col items-center justify-center h-full w-full p-3',
      children: [
        jsx('div', {
          key: 'val',
          className: 'text-2xl font-bold font-mono tabular-nums text-foreground',
          children: String(Math.round(value * 100) / 100) + unit
        }),
        jsxs('div', {
          key: 'bar',
          className: 'w-4/5 h-2 bg-muted rounded mt-3 overflow-hidden',
          children: [
            jsx('div', {
              className: 'h-full rounded ' + barClass,
              style: { width: pct + '%', transition: 'width 0.3s ease' }
            })
          ]
        }),
        jsxs('span', {
          key: 'range',
          className: 'text-[10px] text-muted-foreground mt-1',
          children: [String(min), ' — ', String(max)]
        })
      ]
    });
  }

  return { default: Gauge, Gauge: Gauge };
})();
```

### Template 4: Device Panel (device binding with commands)

```javascript
var DevicePanelWidget = (function() {
  var React = window.React;
  var jsx = window.jsxRuntime.jsx;
  var jsxs = window.jsxRuntime.jsxs;

  function DevicePanel(props) {
    var config = props.config || {};
    var deviceCtx = props.deviceContext;
    var device = deviceCtx && deviceCtx.device;
    var sendCmd = props.sendDeviceCommand;
    var cmdState = React.useState({});
    var cmdLoading = cmdState[0];
    var setCmdLoading = cmdState[1];

    // No device bound
    if (!device) {
      return jsxs('div', {
        className: 'flex flex-col items-center justify-center h-full w-full p-4 border border-border rounded-lg',
        children: [
          jsx('p', { className: 'text-sm text-muted-foreground', children: 'No device bound' }),
          jsx('p', { className: 'text-xs text-muted-foreground mt-1', children: 'Bind a device in config panel' })
        ]
      });
    }

    var vals = device.currentValues || {};
    var online = device.status === 'online';
    var metrics = (deviceCtx.deviceType && deviceCtx.deviceType.metrics) || [];
    var commands = (deviceCtx.deviceType && deviceCtx.deviceType.commands) || [];

    // Metric rows
    var metricRows = metrics.slice(0, 6).map(function(m) {
      var v = vals[m.name];
      var displayVal = v != null ? (typeof v === 'number' ? v.toFixed(1) : String(v)) : '--';
      var u = m.unit ? ' ' + m.unit : '';
      return jsxs('div', {
        key: m.name,
        className: 'flex justify-between text-xs py-1 border-b border-border',
        children: [
          jsx('span', { className: 'text-muted-foreground', children: m.display_name || m.name }),
          jsx('span', { className: 'font-mono tabular-nums text-foreground', children: displayVal + u })
        ]
      });
    });

    // Command buttons
    var cmdButtons = commands.slice(0, 4).map(function(cmd) {
      var isLoading = !!cmdLoading[cmd.name];
      return jsx('button', {
        key: cmd.name,
        className: 'text-xs px-2 py-1 rounded bg-muted text-foreground hover:bg-accent transition-colors disabled:opacity-50',
        onClick: function() {
          if (!sendCmd || isLoading) return;
          var update = {}; update[cmd.name] = true;
          setCmdLoading(function(prev) { return Object.assign({}, prev, update); });
          sendCmd(cmd.name).then(function() {
            var u = {}; u[cmd.name] = false;
            setCmdLoading(function(prev) { return Object.assign({}, prev, u); });
          }).catch(function() {
            var u = {}; u[cmd.name] = false;
            setCmdLoading(function(prev) { return Object.assign({}, prev, u); });
          });
        },
        disabled: isLoading || !online,
        children: isLoading ? '...' : (cmd.display_name || cmd.name)
      });
    });

    return jsxs('div', {
      className: 'flex flex-col h-full w-full p-3 border border-border rounded-lg overflow-auto',
      children: [
        // Header: name + status
        jsxs('div', { key: 'header', className: 'flex items-center justify-between mb-2', children: [
          jsx('span', { key: 'name', className: 'text-sm font-semibold text-foreground truncate', children: device.name || device.id }),
          jsxs('span', { key: 'status', className: 'flex items-center gap-1', children: [
            jsx('div', { key: 'dot', className: 'h-1.5 w-1.5 rounded-full ' + (online ? 'bg-success' : 'bg-muted-foreground') }),
            jsx('span', { key: 'label', className: 'text-[10px] ' + (online ? 'text-success' : 'text-muted-foreground'), children: online ? 'Online' : 'Offline' })
          ]})
        ]}),
        // Metrics
        jsx('div', { className: 'flex-1', children: metricRows }),
        // Commands
        commands.length > 0 ? jsx('div', { className: 'flex gap-1 flex-wrap mt-2', children: cmdButtons }) : null
      ]
    });
  }

  return { default: DevicePanel, DevicePanel: DevicePanel };
})();
```

## manifest.json Complete Structure

```json
{
  "id": "my-widget",
  "name": {"en": "My Widget", "zh": "我的组件"},
  "description": {"en": "Widget description", "zh": "组件描述"},
  "icon": "activity",
  "category": "custom",
  "version": "1.0.0",
  "author": "Author Name",
  "global_name": "MyWidget",
  "export_name": "default",
  "size_constraints": {
    "min_w": 2, "min_h": 2,
    "default_w": 4, "default_h": 3,
    "max_w": 12, "max_h": 8
  },
  "has_data_source": true,
  "max_data_sources": 1,
  "has_device_binding": false,
  "device_type_filter": [],
  "config_schema": {
    "type": "object",
    "properties": {
      "label": {"type": "string", "title": "Display Label", "default": ""}
    }
  },
  "default_config": {"label": ""}
}
```

**Key fields:**
- `global_name`: Must match the IIFE variable name in bundle.js (e.g., `var MyWidget = ...` → `"global_name": "MyWidget"`)
- `export_name`: Usually `"default"`
- `has_data_source`: `true` if widget needs live metric data
- `has_device_binding`: `true` if widget binds to a specific device (provides `deviceContext`)
- `device_type_filter`: Array of device type IDs to filter binding (e.g., `["ne101_camera"]`)
- `config_schema`: JSON Schema for user-configurable options
- `default_config`: Default values for config
- `size_constraints`: Grid units (12-column grid)

## Common Errors & Solutions

| Error | Cause | Solution |
|-------|-------|----------|
| Widget not rendering | `global_name` mismatch | manifest `global_name` must match the IIFE variable name |
| Blank widget | Missing return | Component must return jsx/jsxRuntime call |
| "React is not defined" | Wrong global access | Use `window.React` not `React` |
| "jsxRuntime is not defined" | Wrong runtime access | Use `window.jsxRuntime.jsx` and `window.jsxRuntime.jsxs` |
| Data not showing | Wrong dataSource access | Use `props.dataSource.value` for single, `.timeSeries` for chart |
| "unique key prop" warning | Array children missing keys | Use `jsxs` + add `key` prop to every child in arrays |
| Style not working | Hardcoded colors | Use Tailwind classes or CSS variables, never `#color` |
| Widget too large | Bundle > 50KB | No external libraries, keep code minimal |
| Device panel empty | No device bound | Handle `!device` case with placeholder |
