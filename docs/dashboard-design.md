# User-Customizable Dashboard Design

## Overview

A flexible widget-based dashboard for NeoMind with **two layers of components**:

1. **Generic IoT Components** - Data-agnostic building blocks for any use case
2. **Business Components** - Pre-configured components for NeoMind entities (Agent, Device, Rule, etc.)

## Design Principles

1. **Flexibility**: Generic components work with any data source
2. **Convenience**: Business components provide one-click setup for common scenarios
3. **Composability**: Mix generic and business components freely
4. **Simplicity**: Users can create/modify dashboards with just a few clicks
5. **Real-time**: All widgets update automatically via WebSocket/SSE events
6. **Agent-First**: AI Agent components are highlighted as core features
7. **No New Backend Required**: All widgets use existing APIs

---

## Widget Library - Generic IoT Components

### 📊 Indicator Components

Single-value displays for any metric or status.

| Component | Description | Use Cases |
|-----------|-------------|-----------|
| `value-card` | Big number display with label | Temperature, humidity, power usage, any numeric value |
| `status-badge` | Status indicator (color + text) | Online/Offline, Active/Inactive, Open/Closed |
| `progress-ring` | Circular progress indicator | Battery level, completion percentage, fill level |
| `trend-value` | Value with trend arrow | Stock prices, energy consumption trends |
| `sparkline` | Mini chart in a card | Quick history view of any metric |

### 📈 Chart Components

Standard chart types for data visualization.

| Component | Description | Use Cases |
|-----------|-------------|-----------|
| `line-chart` | Time series line chart | Temperature history, power consumption over time |
| `area-chart` | Filled area chart | Cumulative values, volume over time |
| `bar-chart` | Vertical/horizontal bars | Comparing values across devices/categories |
| `gauge-chart` | Gauge/meter display (0-100%) | Battery, CPU usage, tank level |
| `pie-chart` | Pie/donut chart | Distribution by category |
| `heatmap` | Color-coded grid | Temperature across zones, activity patterns |

### 📋 List Components

Tabular displays for collections of items.

| Component | Description | Use Cases |
|-----------|-------------|-----------|
| `data-table` | Sortable, filterable table | Device list, event log, any tabular data |
| `card-list` | Card-based list | Device cards, agent cards with status |
| `status-list` | Compact status list | Quick overview of many items |
| `log-feed` | Scrolling log view | Event feed, activity log, debug output |

### 🎛️ Control Components

Interactive elements for user input/action.

| Component | Description | Use Cases |
|-----------|-------------|-----------|
| `toggle-switch` | On/off toggle | Enable/disable rules, device control |
| `slider` | Numeric slider | Set value within range (brightness, setpoint) |
| `button-group` | Multiple action buttons | Command execution, mode selection |
| `dropdown` | Select from options | Scene selection, device selection |
| `input-field` | Text/number input | Send commands, configure values |

### 📦 Container Components

Layout and grouping widgets.

| Component | Description | Use Cases |
|-----------|-------------|-----------|
| `tabs` | Tabbed content | Switch between related views |
| `accordion` | Collapsible sections | Organize related widgets |
| `grid` | Fixed grid layout | Dashboard sections |
| `divider` | Visual separator | Group separation |

### 🔔 Notification Components

Alert and status display.

| Component | Description | Use Cases |
|-----------|-------------|-----------|
| `alert-banner` | Alert notification bar | Critical alerts, warnings |
| `badge-counter` | Number badge | Unread count, error count |
| `status-panel` | Multi-status overview | System health summary |

---

## Business Components - NeoMind Specific

Pre-configured components for NeoMind entities. These are built on top of generic components with preset data sources and styling.

### 🤖 AI Agent Components (Core Feature)

Agent-focused components for monitoring and interacting with AI Agents.

| Component | Description | Built From | Data Source |
|-----------|-------------|-----------|-------------|
| `agent-status-card` | Agent status with last run info | value-card + status-badge | `GET /api/agents` |
| `agent-activity-list` | Recent agent executions | card-list | `GET /api/agents/:id/executions` |
| `agent-execution-chart` | Agent execution timeline | line-chart | `GET /api/agents/:id/executions` |
| `agent-memory-view` | Learned patterns display | data-table | `GET /api/agents/:id/memory` |
| `agent-stats` | Success rate, avg duration | stat-card × 2 | `GET /api/agents/:id/stats` |
| `agent-chat` | Quick agent execution | custom form | `POST /api/agents/:id/execute` |
| `agent-conversation` | Conversation history | log-feed | `GET /api/agents/:id` |

**Agent Status Card** - Shows agent name, status (Active/Paused/Error), last execution time, and execution counts:

```
┌─────────────────────────────────────┐
│ 🤖 Temperature Monitor Agent       │
│                                     │
│ Status: ● Active                    │
│ Last Run: 5 minutes ago             │
│ Executions: 156 (Success: 94%)      │
│                                    │
│ [▶ Execute Now]  [⚙ Configure]     │
└─────────────────────────────────────┘
```

**Agent Activity List** - Recent executions with status indicators:

```
┌─────────────────────────────────────┐
│ Recent Executions                   │
│ ├ ✅ 10:23 - Completed (2.3s)       │
│ ├ ✅ 09:50 - Completed (1.8s)       │
│ ├ ✅ 09:15 - Completed (2.1s)       │
│ └ ⚠️ 08:30 - Had warnings (3.2s)    │
└─────────────────────────────────────┘
```

**Agent Chat** - Quick prompt input for on-demand execution:

```
┌─────────────────────────────────────┐
│ Quick Agent Execution               │
│ ┌───────────────────────────────┐  │
│ │ Ask agent to analyze...       │  │
│ └───────────────────────────────┘  │
│                                    │
│ [🚀 Run]  [📜 View History]        │
└─────────────────────────────────────┘
```

### 📱 Device Components

Device monitoring and control components.

| Component | Description | Built From | Data Source |
|-----------|-------------|-----------|-------------|
| `device-grid` | Device cards with status | card-list | `GET /api/devices` |
| `device-telemetry` | Current metrics | value-card × n | `GET /api/devices/:id/telemetry/summary` |
| `device-control` | Toggle/slider for device | toggle + slider | `GET /api/devices/:id/state` |
| `device-health` | Online/offline summary | status-panel | `GET /api/devices` |

### 📜 Rule Components

Automation rule monitoring.

| Component | Description | Built From | Data Source |
|-----------|-------------|-----------|-------------|
| `rule-status-grid` | Rule enable/disable cards | card-list | `GET /api/rules` |
| `rule-trigger-log` | Recent rule triggers | log-feed | `GET /api/rules/:id/history` |
| `rule-tester` | Test rule with current data | custom form | `POST /api/rules/:id/test` |

### 🔄 Transform Components

Data transform monitoring.

| Component | Description | Built From | Data Source |
|-----------|-------------|-----------|-------------|
| `transform-list` | All transforms with status | data-table | `GET /api/automations?type=transform` |
| `virtual-metrics` | Generated metrics list | card-list | `GET /api/automations/transforms/metrics` |

### ⚠️ Alert Components

Alert management.

| Component | Description | Built From | Data Source |
|-----------|-------------|-----------|-------------|
| `alert-panel` | Active alerts with severity | alert-banner + list | `GET /api/alerts` |
| `alert-by-severity` | Count by severity | badge-counter × 4 | `GET /api/alerts` |
| `alert-timeline` | Alert history timeline | log-feed | `GET /api/alerts` |

### 🧠 Decision Components

AI decision tracking.

| Component | Description | Built From | Data Source |
|-----------|-------------|-----------|-------------|
| `decision-list` | Recent AI decisions | card-list | `GET /api/decisions` |
| `decision-detail` | Decision with reasoning | custom panel | `GET /api/decisions/:id` |
| `decision-stats` | Decision trends | stat-card × n | `GET /api/decisions/stats` |

---

## Component Configuration Schema

All components follow a generic configuration pattern:

```typescript
interface WidgetComponent {
  id: string                    // Unique widget ID
  type: ComponentType           // Component type
  title?: string                // Optional title
  dataSource: DataSource        // Data source configuration
  display: DisplayConfig        // Display/styling options
  actions?: ActionConfig[]      // Optional actions
}

// Generic data source - works with any system data
interface DataSource {
  type: 'api' | 'websocket' | 'static' | 'computed'
  endpoint?: string             // API endpoint path
  transform?: string            // JS expression to transform data
  refresh?: number              // Refresh interval (seconds)
  params?: Record<string, unknown>  // Query parameters
  staticValue?: unknown         // For static/manual input
}

// Generic display configuration
interface DisplayConfig {
  // Formatting
  format?: string               // Number format (e.g., "0.00", "#,###")
  unit?: string                 // Unit suffix (°C, %, kWh, etc.)
  prefix?: string               // Prefix (currency symbols, etc.)

  // Colors
  color?: string                // Base color
  colorScale?: ColorScale       // Conditional coloring

  // Ranges
  min?: number
  max?: number
  thresholds?: Threshold[]      // Alert thresholds

  // Layout
  size?: 'sm' | 'md' | 'lg'
  density?: 'compact' | 'comfortable' | 'spacious'

  // Chart specific
  showLegend?: boolean
  showGrid?: boolean
  timeRange?: string            // "1h", "24h", "7d", "30d"
  aggregation?: string          // "avg", "min", "max", "sum"
}

// Color scale for conditional formatting
interface ColorScale {
  type: 'threshold' | 'gradient' | 'category'
  stops: ColorStop[]
}

interface ColorStop {
  value: number | string
  color: string
}

// Threshold for alerts/warnings
interface Threshold {
  value: number
  operator: '>' | '<' | '=' | '>=' | '<='
  color: string
  icon?: string
}
```

---

## Component Examples

### Example 1: Temperature Display

```json
{
  "type": "value-card",
  "title": "Living Room Temperature",
  "dataSource": {
    "type": "api",
    "endpoint": "/devices/sensor-001/telemetry/summary",
    "transform": "data.summary.temperature",
    "refresh": 10
  },
  "display": {
    "format": "0.1",
    "unit": "°C",
    "size": "lg",
    "colorScale": {
      "type": "threshold",
      "stops": [
        { "value": 18, "color": "#3b82f6" },
        { "value": 22, "color": "#22c55e" },
        { "value": 26, "color": "#f59e0b" },
        { "value": 30, "color": "#ef4444" }
      ]
    }
  }
}
```

### Example 2: Device Status Table

```json
{
  "type": "data-table",
  "title": "All Devices",
  "dataSource": {
    "type": "api",
    "endpoint": "/devices",
    "refresh": 30
  },
  "display": {
    "columns": [
      { "key": "name", "label": "Name", "width": "30%" },
      { "key": "status", "label": "Status", "width": "20%" },
      { "key": "last_seen", "label": "Last Seen", "width": "25%" },
      { "key": "current_values", "label": "Value", "width": "25%" }
    ],
    "sortable": true,
    "filterable": true,
    "rowAction": {
      "type": "navigate",
      "path": "/devices/:id"
    }
  }
}
```

### Example 3: Power Usage Chart

```json
{
  "type": "area-chart",
  "title": "Power Consumption (24h)",
  "dataSource": {
    "type": "api",
    "endpoint": "/devices/smart-meter/metrics/power/data",
    "params": {
      "start": "now-24h",
      "end": "now",
      "agg": "avg"
    }
  },
  "display": {
    "unit": "kW",
    "color": "#8b5cf6",
    "showGrid": true,
    "timeRange": "24h"
  }
}
```

### Example 4: Control Toggle

```json
{
  "type": "toggle-switch",
  "title": "Office Lights",
  "dataSource": {
    "type": "api",
    "endpoint": "/devices/light-002/state",
    "refresh": 5
  },
  "display": {
    "onLabel": "On",
    "offLabel": "Off",
    "onColor": "#22c55e",
    "offColor": "#6b7280"
  },
  "actions": [
    {
      "type": "api-call",
      "method": "POST",
      "endpoint": "/devices/light-002/command/toggle",
      "confirm": false
    }
  ]
}
```

### Example 5: Multi-Device Status

```json
{
  "type": "status-list",
  "title": "Device Health",
  "dataSource": {
    "type": "api",
    "endpoint": "/devices",
    "transform": "data.filter(d => d.online !== true)"
  },
  "display": {
    "itemTemplate": "{name}: {status}",
    "statusMapping": {
      "online": { "color": "green", "icon": "check-circle" },
      "offline": { "color": "red", "icon": "x-circle" },
      "error": { "color": "orange", "icon": "alert-triangle" }
    }
  }
}
```

---

## Generic vs Business Components

### Comparison: Agent Status Display

**Using Generic Components** (requires more configuration):

```json
{
  "type": "value-card",
  "title": "Temperature Agent",
  "dataSource": {
    "type": "api",
    "endpoint": "/agents/temp-monitor",
    "transform": "data.execution_count"
  },
  "display": {
    "format": "#,###",
    "unit": "runs"
  }
}
```

**Using Business Component** (simplified configuration):

```json
{
  "type": "agent-status-card",
  "agentId": "temp-monitor"
}
```

### Business Component Configuration

Business components have minimal configuration - just select the entity:

```typescript
// Agent components
interface AgentStatusCardConfig {
  agentId: string              // Agent ID
  showExecuteButton?: boolean   // Show execute button
  showStats?: boolean           // Show execution stats
}

interface AgentActivityListConfig {
  agentId: string
  limit?: number                // Max executions (default: 10)
  showStatus?: boolean          // Show execution status
}

interface AgentChatConfig {
  agentId: string
  placeholder?: string          // Input placeholder
  showHistory?: boolean         // Show conversation history
}

// Device components
interface DeviceGridConfig {
  filter?: {
    onlineOnly?: boolean
    deviceType?: string
  }
  sortBy?: 'name' | 'status' | 'lastSeen'
  showControls?: boolean
}

interface DeviceTelemetryConfig {
  deviceId: string
  metrics: string[]             // Metrics to show
  layout?: 'grid' | 'list'
}

// Rule components
interface RuleStatusGridConfig {
  enabledOnly?: boolean
  showTriggerCount?: boolean
}

interface RuleTriggerLogConfig {
  ruleId: string
  limit?: number
}
```

### When to Use Which?

| Scenario | Use | Reason |
|----------|-----|--------|
| Show agent status | Business | Pre-configured, one-click setup |
| Show device temperature | Generic or Business | Generic for custom format, Business for quick setup |
| Custom data display | Generic | Flexibility to show any data |
| Monitor system entities | Business | Optimized for NeoMind entities |
| Build custom views | Generic | Mix and match for unique needs |

### Component Library UI

The component picker is organized into tabs:

```
┌─────────────────────────────────────────────────────────────┐
│ Add Component                                   ╳          │
├─────────────────────────────────────────────────────────────┤
│ [🤖 Business]  [📊 Generic]  [🔔 Notifications]              │
├─────────────────────────────────────────────────────────────┤
│ Business Components                                            │
│                                                               │
│ AI Agents ⭐                                                   │
│   ┌─────────────────────┐  ┌─────────────────────┐        │
│   │ Agent Status Card   │  │ Agent Activity List │        │
│   │ Quick agent overview│  │ Recent executions   │        │
│   └─────────────────────┘  └─────────────────────┘        │
│   ┌─────────────────────┐  ┌─────────────────────┐        │
│   │ Agent Chat          │  │ Agent Memory View   │        │
│   │ Execute on-demand   │  │ Learned patterns    │        │
│   └─────────────────────┘  └─────────────────────┘        │
│                                                               │
│ Devices                                                      │
│   ┌─────────────────────┐  ┌─────────────────────┐        │
│   │ Device Grid         │  │ Device Telemetry    │        │
│   │ All devices status  │  │ Current metrics      │        │
│   └─────────────────────┘  └─────────────────────┘        │
│                                                               │
│ Rules & Automation                                           │
│   ┌─────────────────────┐  ┌─────────────────────┐        │
│   │ Rule Status Grid     │  │ Rule Trigger Log     │        │
│   │ Enable/disable rules│  │ Recent triggers      │        │
│   └─────────────────────┘  └─────────────────────┘        │
└─────────────────────────────────────────────────────────────┘
```

---

## Component Type Reference

### Value Card

```typescript
interface ValueCardConfig extends DisplayConfig {
  // Inherited: format, unit, prefix, colorScale, size
  showTrend?: boolean          // Show trend indicator
  trendPeriod?: string         // "1h", "24h", "7d"
  showSparkline?: boolean      // Mini chart
  icon?: string                // Lucide icon name
}
```

### Status Badge

```typescript
interface StatusBadgeConfig extends DisplayConfig {
  // Inherited: colorScale, size
  statusField?: string         // Field to read status from
  statusMapping?: StatusMapping
  showIcon?: boolean
}

interface StatusMapping {
  [key: string]: {
    label: string
    color: string
    icon?: string
  }
}
```

### Progress Ring

```typescript
interface ProgressRingConfig extends DisplayConfig {
  // Inherited: min, max, colorScale, size
  showPercentage?: boolean
  thickness?: number           // 1-10
  clockwise?: boolean
}
```

### Line/Area Chart

```typescript
interface LineChartConfig extends DisplayConfig {
  // Inherited: unit, color, showLegend, showGrid, timeRange
  curve?: 'linear' | 'monotone' | 'step'
  fill?: boolean               // True for area chart
  showPoints?: boolean
  multipleSeries?: boolean      // Show multiple data series
  seriesConfig?: SeriesConfig[]
}

interface SeriesConfig {
  key: string
  label: string
  color: string
}
```

### Bar Chart

```typescript
interface BarChartConfig extends DisplayConfig {
  // Inherited: unit, color, showLegend, showGrid
  orientation?: 'vertical' | 'horizontal'
  stacked?: boolean
  groupBy?: string
}
```

### Gauge Chart

```typescript
interface GaugeConfig extends DisplayConfig {
  // Inherited: min, max, colorScale
  showValue?: boolean
  zones?: GaugeZone[]
}

interface GaugeZone {
  from: number
  to: number
  color: string
  label?: string
}
```

### Data Table

```typescript
interface DataTableConfig extends DisplayConfig {
  columns: ColumnConfig[]
  sortable?: boolean
  filterable?: boolean
  paginated?: boolean
  pageSize?: number
  rowAction?: RowAction
  selectable?: boolean
}

interface ColumnConfig {
  key: string
  label: string
  width?: string
  format?: string
  sortable?: boolean
  filterable?: boolean
}

interface RowAction {
  type: 'navigate' | 'dialog' | 'api-call'
  path?: string              // For navigate
  dialog?: string            // For dialog
  method?: string            // For api-call
  endpoint?: string
}
```

### Log Feed

```typescript
interface LogFeedConfig extends DisplayConfig {
  // Inherited: timeRange
  maxItems?: number
  autoScroll?: boolean
  showTimestamp?: boolean
  showSource?: boolean
  level?: string              // Filter by log level
  format?: 'compact' | 'detailed'
}
```

### Toggle Switch

```typescript
interface ToggleConfig extends DisplayConfig {
  // Inherited: onColor, offColor
  onLabel?: string
  offLabel?: string
  size?: 'sm' | 'md' | 'lg'
  loading?: boolean
}
```

### Slider

```typescript
interface SliderConfig extends DisplayConfig {
  // Inherited: min, max
  step?: number
  showValue?: boolean
  marks?: SliderMark[]
  liveUpdate?: boolean        // Update while dragging
}

interface SliderMark {
  value: number
  label?: string
}
```

### Button Group

```typescript
interface ButtonGroupConfig extends DisplayConfig {
  buttons: ButtonConfig[]
  orientation?: 'horizontal' | 'vertical'
  variant?: 'solid' | 'outline' | 'ghost'
}

interface ButtonConfig {
  id: string
  label: string
  icon?: string
  color?: string
  action: ActionConfig
}
```

---

## Data Source Templates

Pre-configured data sources for common NeoMind entities:

```typescript
const DataSourceTemplates = {
  // Device-related
  deviceList: {
    type: 'api',
    endpoint: '/devices',
    description: 'All devices'
  },
  deviceTelemetry: {
    type: 'api',
    endpoint: '/devices/:deviceId/telemetry/summary',
    description: 'Device telemetry summary'
  },
  deviceMetric: {
    type: 'api',
    endpoint: '/devices/:deviceId/metrics/:metric/data',
    description: 'Device metric history'
  },
  deviceState: {
    type: 'api',
    endpoint: '/devices/:deviceId/state',
    description: 'Current device state'
  },

  // Agent-related
  agentList: {
    type: 'api',
    endpoint: '/agents',
    description: 'All AI agents'
  },
  agentExecutions: {
    type: 'api',
    endpoint: '/agents/:agentId/executions',
    description: 'Agent execution history'
  },
  agentStats: {
    type: 'api',
    endpoint: '/agents/:agentId/stats',
    description: 'Agent statistics'
  },

  // Automation-related
  ruleList: {
    type: 'api',
    endpoint: '/rules',
    description: 'All rules'
  },
  transformList: {
    type: 'api',
    endpoint: '/automations?type=transform',
    description: 'All transforms'
  },

  // System
  systemStats: {
    type: 'api',
    endpoint: '/stats/system',
    description: 'System statistics'
  },
  alerts: {
    type: 'api',
    endpoint: '/alerts',
    description: 'All alerts'
  },
  events: {
    type: 'api',
    endpoint: '/events/history',
    description: 'Event history'
  },

  // Real-time
  websocket: {
    type: 'websocket',
    endpoint: '/events/ws',
    description: 'Real-time event stream'
  }
}
```

---

## Preset Widget Templates

Ready-to-use widget combinations:

### Temperature Monitor

```json
{
  "name": "Temperature Monitor",
  "widgets": [
    {
      "type": "value-card",
      "title": "Current",
      "dataSource": { "template": "deviceTelemetry", "params": { "deviceId": "xxx" } },
      "display": { "unit": "°C" }
    },
    {
      "type": "line-chart",
      "title": "History (24h)",
      "dataSource": { "template": "deviceMetric" },
      "display": { "timeRange": "24h" }
    },
    {
      "type": "sparkline",
      "title": "Trend",
      "dataSource": { "template": "deviceMetric" }
    }
  ]
}
```

### Device Control Panel

```json
{
  "name": "Device Control",
  "widgets": [
    {
      "type": "status-badge",
      "title": "Status",
      "dataSource": { "template": "deviceState" }
    },
    {
      "type": "toggle-switch",
      "title": "Power",
      "dataSource": { "template": "deviceState" }
    },
    {
      "type": "slider",
      "title": "Brightness",
      "dataSource": { "template": "deviceState" },
      "display": { "min": 0, "max": 100 }
    }
  ]
}
```

### System Health Overview

```json
{
  "name": "System Health",
  "widgets": [
    {
      "type": "data-table",
      "title": "Devices",
      "dataSource": { "template": "deviceList" }
    },
    {
      "type": "status-list",
      "title": "Agents",
      "dataSource": { "template": "agentList" }
    },
    {
      "type": "alert-banner",
      "title": "Alerts",
      "dataSource": { "template": "alerts" }
    }
  ]
}
```

---

## Dashboard Templates

### Template 1: Overview Dashboard

A general-purpose dashboard showing system health at a glance.

```
┌─────────────────────────────────────────────────────────────┐
│  ┌───────────┐  ┌───────────┐  ┌───────────┐               │
│  │Value Card │  │Value Card │  │Value Card │               │
│  │Devices:12 │  │Online: 10 │  │Rules: 8   │               │
│  └───────────┘  └───────────┘  └───────────┘               │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────────────────────────────────────────────┐    │
│  │  Data Table (All Devices)                           │    │
│  │  ┌──────────┬─────────┬─────────┬──────────────┐     │    │
│  │  │ Name     │ Status  │ Value   │ Last Seen    │     │    │
│  │  ├──────────┼─────────┼─────────┼──────────────┤     │    │
│  │  │ Sensor 1 │ Online  │ 22.5°C  │ 10:23:15     │     │    │
│  │  │ Sensor 2 │ Online  │ 45%     │ 10:23:10     │     │    │
│  │  │ Switch 1 │ Online  │ ON      │ 10:22:00     │     │    │
│  │  └──────────┴─────────┴─────────┴──────────────┘     │    │
│  └─────────────────────────────────────────────────────┘    │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────────────────────────────────────────────┐    │
│  │  Log Feed (Recent Events)                           │    │
│  │  [10:23] DeviceMetric: sensor1/temp = 22.5          │    │
│  │  [10:22] RuleTriggered: temp-alert                  │    │
│  │  [10:21] DeviceOnline: sensor2                      │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

**Components Used:**
- `value-card` × 3 - Device counts
- `data-table` - Device list
- `log-feed` - Event stream

### Template 2: Energy Monitor

Monitor energy-consuming devices with generic components.

```
┌─────────────────────────────────────────────────────────────┐
│  ┌─────────────────────────────────────────────────────┐    │
│  │  Area Chart (Power 24h)                             │    │
│  │  ╭────────────────────────────────────────────────╮  │    │
│  │  │ 3.0 ──╮                                         │  │    │
│  │  │ 2.5   │ ╭──╮                                   │  │    │
│  │  │ 2.0   ╭─╯  ╰──╮                                │  │    │
│  │  │ 1.5  ╭─╯      ╰──╮                             │  │    │
│  │  │ 1.0 ──╯          ╰─╮                           │  │    │
│  │  │ 0.5                 ╰───                       │  │    │
│  │  └────────────────────────────────────────────────╯  │    │
│  └─────────────────────────────────────────────────────┘    │
├─────────────────────────────────────────────────────────────┤
│  ┌───────────┐  ┌───────────┐  ┌───────────┐               │
│  │Value Card │  │Value Card │  │Value Card │               │
│  │2.4 kW     │  │Today:     │  │This Week: │               │
│  │           │  │42.5 kWh   │  │287 kWh    │               │
│  └───────────┘  └───────────┘  └───────────┘               │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────────────────────────────────────────────┐    │
│  │  Bar Chart (By Device)                              │    │
│  │  HVAC  ████ 1.2 kW                                 │    │
│  │  Lights ██ 0.4 kW                                  │    │
│  │  Other  ███ 0.8 kW                                 │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

**Components Used:**
- `area-chart` - Power over time
- `value-card` × 3 - Current and cumulative usage
- `bar-chart` - Per-device breakdown

### Template 3: Device Control

Interactive control panel using generic components.

```
┌─────────────────────────────────────────────────────────────┐
│  ┌─────────────────────────────────────────────────────┐    │
│  │  Tabs: [Living Room] [Bedroom] [Kitchen]           │    │
│  ├─────────────────────────────────────────────────────┤    │
│  │  ┌───────────┐  Status Badge: Online               │    │
│  │  │Toggle     │                                      │    │
│  │  │[ON/OFF]   │  ┌─────────────────────┐             │    │
│  │  └───────────┘  │ Slider: Brightness  │             │    │
│  │                 │ ╺━━━━━━━━━━━━━━━━━  │ 75%         │    │
│  │                 └─────────────────────┘             │    │
│  │  ┌─────────────────────┐                            │    │
│  │  │ Value Card: 22.5°C  │  ┌───────────┐            │    │
│  │  └─────────────────────┘  │Toggle     │            │    │
│  │                           │[ON/OFF]   │            │    │
│  │  ┌─────────────────────┐  └───────────┘            │    │
│  │  │ Gauge: Humidity     │                            │    │
│  │  │      ╺━━━━━━━━━    │ 45%                         │    │
│  │  └─────────────────────┘                            │    │
│  └─────────────────────────────────────────────────────┘    │
│  ┌─────────────────────────────────────────────────────┐    │
│  │  Button Group: [Scene: Day] [Scene: Evening] [Off] │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

**Components Used:**
- `tabs` - Room selection
- `status-badge` - Device status
- `toggle-switch` × 2 - Power control
- `slider` - Brightness
- `value-card` - Temperature
- `gauge-chart` - Humidity
- `button-group` - Scene selection

### Template 4: Status Dashboard

Multi-entity status monitoring.

```
┌─────────────────────────────────────────────────────────────┐
│  ┌─────────────────────────────────────────────────────┐    │
│  │  Status Panel (System Health)                       │    │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────┐            │    │
│  │  │Devices   │ │Agents    │ │Rules     │            │    │
│  │  │●10/12    │ │●3/4      │ │●7/8      │            │    │
│  │  └──────────┘ └──────────┘ └──────────┘            │    │
│  └─────────────────────────────────────────────────────┘    │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────────────────────────────────────────────┐    │
│  │  Status List (All Entities)                        │    │
│  │  ● Sensor 1 - Online (22.5°C)                       │    │
│  │  ● Sensor 2 - Online (45%)                          │    │
│  │  ○ Switch 1 - Offline                               │    │
│  │  ● Agent 1 - Active (Last run: 5m ago)             │    │
│  │  ○ Agent 2 - Paused                                 │    │
│  └─────────────────────────────────────────────────────┘    │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────────────────────────────────────────────┐    │
│  │  Alert Banner (Active Alerts)                       │    │
│  │  ⚠ Warning: Sensor 3 offline (2h ago)              │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

**Components Used:**
- `status-panel` - System overview
- `status-list` - All entity status
- `alert-banner` - Active alerts

### Template 5: Blank Canvas

Empty dashboard for user to build from scratch.

```
┌─────────────────────────────────────────────────────────────┐
│  [+ Add Widget]                                             │
│                                                             │
│  Drag widgets here or click "Add Widget" to start          │
│                                                             │
│  ╔═══════════════════════════════════════════════════════╗   │
│  ║                                                       ║   │
│  ║                                                       ║   │
│  ║                                                       ║   │
│  ╚═══════════════════════════════════════════════════════╝   │
└─────────────────────────────────────────────────────────────┘
```

---

## File Structure

```
web/src/
├── pages/
│   └── dashboard.tsx                 # Main dashboard page
├── components/
│   └── dashboard/
│       ├── Dashboard.tsx             # Main dashboard container
│       ├── DashboardGrid.tsx         # Grid layout component
│       ├── ComponentLibrary.tsx      # Component picker (generic + business)
│       ├── ComponentConfig.tsx       # Component configuration panel
│       ├── DashboardTemplates.tsx    # Template selection
│       ├── DashboardToolbar.tsx      # Dashboard controls
│       │
│       ├── components/
│       │   ├── WidgetWrapper.tsx     # Common wrapper (resize, delete, config)
│       │   ├── ComponentRegistry.tsx # Component registry with metadata
│       │   └── index.ts              # Export all components
│       │
│       │   ├── generic/              # Generic IoT components (data-agnostic)
│       │   │   ├── indicators/
│       │   │   │   ├── ValueCard.tsx         # Big number display
│       │   │   │   ├── StatusBadge.tsx       # Status indicator
│       │   │   │   ├── ProgressRing.tsx      # Circular progress
│       │   │   │   ├── TrendValue.tsx        # Value with trend
│       │   │   │   └── Sparkline.tsx         # Mini chart
│       │   │   ├── charts/
│       │   │   │   ├── LineChart.tsx         # Time series
│       │   │   │   ├── AreaChart.tsx         # Filled area
│       │   │   │   ├── BarChart.tsx          # Bar chart
│       │   │   │   ├── GaugeChart.tsx        # Gauge/meter
│       │   │   │   ├── PieChart.tsx          # Pie/donut
│       │   │   │   └── Heatmap.tsx           # Color grid
│       │   │   ├── lists/
│       │   │   │   ├── DataTable.tsx         # Sortable table
│       │   │   │   ├── CardList.tsx          # Card-based list
│       │   │   │   ├── StatusList.tsx        # Compact status
│       │   │   │   └── LogFeed.tsx           # Scrolling feed
│       │   │   ├── controls/
│       │   │   │   ├── ToggleSwitch.tsx      # On/off toggle
│       │   │   │   ├── Slider.tsx            # Numeric slider
│       │   │   │   ├── ButtonGroup.tsx       # Action buttons
│       │   │   │   ├── Dropdown.tsx          # Select
│       │   │   │   └── InputField.tsx        # Text input
│       │   │   ├── containers/
│       │   │   │   ├── Tabs.tsx              # Tabbed content
│       │   │   │   ├── Accordion.tsx         # Collapsible
│       │   │   │   ├── Grid.tsx              # Fixed layout
│       │   │   │   └── Divider.tsx           # Separator
│       │   │   └── notifications/
│       │   │       ├── AlertBanner.tsx       # Alert bar
│       │   │       ├── BadgeCounter.tsx      # Number badge
│       │   │       └── StatusPanel.tsx       # Status overview
│       │
│       │   └── business/             # NeoMind business components (pre-configured)
│       │       ├── agents/              # ⭐ AI Agent components
│       │       │   ├── AgentStatusCard.tsx     # Agent overview with execute button
│       │       │   ├── AgentActivityList.tsx   # Recent executions
│       │       │   ├── AgentExecutionChart.tsx # Execution timeline
│       │       │   ├── AgentMemoryView.tsx    # Learned patterns
│       │       │   ├── AgentStats.tsx          # Success rate, duration
│       │       │   ├── AgentChat.tsx           # Quick execution
│       │       │   └── AgentConversation.tsx   # Chat history
│       │       ├── devices/
│       │       │   ├── DeviceGrid.tsx          # Device cards with status
│       │       │   ├── DeviceTelemetry.tsx     # Current metrics
│       │       │   ├── DeviceControl.tsx       # Toggle/slider controls
│       │       │   └── DeviceHealth.tsx        # Online/offline summary
│       │       ├── rules/
│       │       │   ├── RuleStatusGrid.tsx      # Rule enable/disable
│       │       │   ├── RuleTriggerLog.tsx      # Recent triggers
│       │       │   └── RuleTester.tsx         # Test with current data
│       │       ├── transforms/
│       │       │   ├── TransformList.tsx      # All transforms
│       │       │   └── VirtualMetrics.tsx      # Generated metrics
│       │       ├── alerts/
│       │       │   ├── AlertPanel.tsx          # Active alerts
│       │       │   ├── AlertBySeverity.tsx     # Count by severity
│       │       │   └── AlertTimeline.tsx       # Alert history
│       │       └── decisions/
│       │           ├── DecisionList.tsx       # Recent decisions
│       │           ├── DecisionDetail.tsx     # Decision with reasoning
│       │           └── DecisionStats.tsx      # Decision trends
│       │
│       └── hooks/
│           ├── useComponentData.ts   # Generic data fetching
│           └── useComponentState.ts  # Component state management
│
├── store/
│   └── slices/
│       └── dashboardSlice.ts        # Redux state management
├── hooks/
│   ├── useDashboard.ts               # Dashboard CRUD hooks
│   └── useDataSource.ts              # Generic data source hook
├── lib/
│   ├── dashboard-api.ts              # Dashboard API client
│   └── data-sources.ts              # Data source templates
└── types/
    └── dashboard.ts                  # Dashboard type definitions
```

---

## Data Source Registry

Central registry of available data sources for components:

```typescript
// lib/data-sources.ts
export const DataSourceRegistry = {
  // Device sources
  'device:list': {
    endpoint: '/devices',
    description: 'All devices',
    category: 'device'
  },
  'device:telemetry': {
    endpoint: '/devices/:id/telemetry/summary',
    description: 'Device telemetry',
    category: 'device'
  },
  'device:state': {
    endpoint: '/devices/:id/state',
    description: 'Device state',
    category: 'device'
  },
  'device:metric': {
    endpoint: '/devices/:id/metrics/:metric/data',
    description: 'Metric history',
    category: 'device'
  },

  // Agent sources
  'agent:list': {
    endpoint: '/agents',
    description: 'All agents',
    category: 'agent'
  },
  'agent:executions': {
    endpoint: '/agents/:id/executions',
    description: 'Agent executions',
    category: 'agent'
  },
  'agent:memory': {
    endpoint: '/agents/:id/memory',
    description: 'Agent memory',
    category: 'agent'
  },

  // Automation sources
  'rule:list': {
    endpoint: '/rules',
    description: 'All rules',
    category: 'automation'
  },
  'automation:list': {
    endpoint: '/automations',
    description: 'All automations',
    category: 'automation'
  },

  // System sources
  'alert:list': {
    endpoint: '/alerts',
    description: 'All alerts',
    category: 'system'
  },
  'event:history': {
    endpoint: '/events/history',
    description: 'Event history',
    category: 'system'
  },
  'stats:system': {
    endpoint: '/stats/system',
    description: 'System stats',
    category: 'system'
  },
  'command:list': {
    endpoint: '/commands',
    description: 'Command history',
    category: 'system'
  },

  // Real-time
  'websocket:events': {
    endpoint: '/events/ws',
    description: 'Real-time events',
    category: 'realtime'
  }
}
```

---

## Implementation Phases

### Phase 1: Core Infrastructure (Week 1)

**Goal**: Set up the basic dashboard framework

1. **Type Definitions**
   - Define all dashboard types in `types/dashboard.ts`
   - Create component registry with metadata
   - Define generic data source types

2. **State Management**
   - Create `dashboardSlice` for Redux
   - Implement dashboard CRUD operations

3. **Grid Layout**
   - Integrate `react-grid-layout`
   - Implement responsive breakpoints
   - Add drag-and-drop support

4. **Basic UI**
   - Dashboard page shell
   - Toolbar (edit mode, add component, templates)
   - Component wrapper (resize, delete, config)

**Deliverable**: Working dashboard with draggable empty components

---

### Phase 2: Generic Components - Indicators (Week 2)

**Goal**: Implement indicator components

1. **Value Display**
   - `ValueCard` - Big number with formatting
   - `StatusBadge` - Status with color mapping
   - `ProgressRing` - Circular progress

2. **Trend Indicators**
   - `TrendValue` - Value with trend arrow
   - `Sparkline` - Mini chart

**Deliverable**: Basic numeric/status display components

---

### Phase 3: Generic Components - Charts (Week 2-3)

**Goal**: Implement chart components

1. **Charts**
   - `LineChart` - Time series
   - `AreaChart` - Filled area
   - `BarChart` - Vertical/horizontal bars
   - `GaugeChart` - Gauge/meter

2. **Advanced Charts**
   - `PieChart` - Distribution
   - `Heatmap` - Color grid

**Deliverable**: Full charting capabilities

---

### Phase 4: Generic Components - Lists & Controls (Week 3)

**Goal**: Implement interactive components

1. **Lists**
   - `DataTable` - Sortable, filterable
   - `StatusList` - Compact status
   - `LogFeed` - Scrolling feed
   - `CardList` - Card-based list

2. **Controls**
   - `ToggleSwitch` - On/off
   - `Slider` - Numeric input
   - `ButtonGroup` - Actions
   - `Dropdown` - Selection

**Deliverable**: Interactive and data display components

---

### Phase 5: Containers & Templates (Week 4)

**Goal**: Layout and template system

1. **Containers**
   - `Tabs` - Tabbed content
   - `Accordion` - Collapsible
   - `Grid` - Fixed layout

2. **Templates**
   - Implement predefined templates
   - Template selection dialog
   - One-click apply

3. **Persistence**
   - Save/load dashboard
   - Default dashboard handling

4. **Real-time**
   - WebSocket integration
   - Auto-refresh options

**Deliverable**: Production-ready customizable dashboard

---

## Component Type Summary

| Category | Components | Data Agnostic |
|----------|------------|---------------|
| **Indicators** | value-card, status-badge, progress-ring, trend-value, sparkline | ✅ |
| **Charts** | line-chart, area-chart, bar-chart, gauge-chart, pie-chart, heatmap | ✅ |
| **Lists** | data-table, card-list, status-list, log-feed | ✅ |
| **Controls** | toggle-switch, slider, button-group, dropdown, input-field | ✅ |
| **Containers** | tabs, accordion, grid, divider | ✅ |
| **Notifications** | alert-banner, badge-counter, status-panel | ✅ |

**Key Benefit**: Each component can display data from ANY source - devices, agents, rules, alerts, or custom data. Users mix and match to build their views.

---

## API Requirements (Backend)

Dashboard persistence endpoints to add:

```
# Dashboard management
GET    /api/dashboards              # List user dashboards
POST   /api/dashboards              # Create dashboard
GET    /api/dashboards/:id          # Get dashboard
PUT    /api/dashboards/:id          # Update dashboard
DELETE /api/dashboards/:id          # Delete dashboard
POST   /api/dashboards/:id/default  # Set as default

# Templates (read-only)
GET    /api/dashboards/templates    # List templates
GET    /api/dashboards/templates/:id # Get template
```

**Storage**: Use existing `settings.redb` - no new database needed

---

## Dependencies

```json
{
  "dependencies": {
    "react-grid-layout": "^1.4.4",
    "recharts": "^2.10.0",
    "lucide-react": "^0.300.0"
  }
}
```

---

## Generic Component Example

```tsx
// Example: ValueCard component (data-agnostic)
interface ValueCardProps {
  id: string
  title?: string
  dataSource: DataSource
  display: DisplayConfig
}

export function ValueCard({ id, title, dataSource, display }: ValueCardProps) {
  // Generic data hook - works with any data source
  const { data, isLoading, error } = useDataSource(dataSource)

  if (isLoading) return <Skeleton />
  if (error) return <ErrorState error={error} />

  // Extract value using configured path (e.g., "data.summary.temperature")
  const value = extractValue(data, dataSource.transform || 'data')

  // Apply formatting from display config
  const formattedValue = formatValue(value, display.format, display.unit)

  // Apply color scale if configured
  const color = applyColorScale(value, display.colorScale)

  return (
    <div className="widget value-card" style={{ color }}>
      {title && <h3>{title}</h3>}
      <div className="value" style={{ fontSize: display.size === 'lg' ? '2rem' : '1.5rem' }}>
        {display.prefix && <span>{display.prefix}</span>}
        {formattedValue}
      </div>
      {display.showTrend && <TrendIndicator value={value} period={display.trendPeriod} />}
      {display.showSparkline && <Sparkline data={data} />}
    </div>
  )
}

// Usage examples with different data sources:

// Example 1: Device temperature
<ValueCard
  title="Living Room"
  dataSource={{
    type: 'api',
    endpoint: '/devices/sensor-001/telemetry/summary',
    transform: 'data.summary.temperature',
    refresh: 10
  }}
  display={{
    format: '0.1',
    unit: '°C',
    size: 'lg',
    colorScale: {
      type: 'threshold',
      stops: [
        { value: 18, color: '#3b82f6' },
        { value: 22, color: '#22c55e' },
        { value: 26, color: '#f59e0b' },
        { value: 30, color: '#ef4444' }
      ]
    }
  }}
/>

// Example 2: Agent execution count
<ValueCard
  title="Agent Runs Today"
  dataSource={{
    type: 'api',
    endpoint: '/agents/agent-001/stats',
    transform: 'data.total_executions'
  }}
  display={{
    format: '#,###',
    unit: 'runs',
    size: 'md'
  }}
/>

// Example 3: Static/manual value
<ValueCard
  title="Setpoint"
  dataSource={{
    type: 'static',
    staticValue: 22
  }}
  display={{
    format: '0.1',
    unit: '°C'
  }}
/>
```

---

## Component Registry

```typescript
// components/ComponentRegistry.tsx
export const ComponentRegistry = {
  // Indicators
  'value-card': {
    name: 'Value Card',
    category: 'indicators',
    icon: 'Hash',
    description: 'Display a single numeric value',
    configSchema: ValueCardConfigSchema,
    component: ValueCard
  },
  'status-badge': {
    name: 'Status Badge',
    category: 'indicators',
    icon: 'Badge',
    description: 'Status indicator with color',
    configSchema: StatusBadgeConfigSchema,
    component: StatusBadge
  },
  'progress-ring': {
    name: 'Progress Ring',
    category: 'indicators',
    icon: 'Circle',
    description: 'Circular progress indicator',
    configSchema: ProgressRingConfigSchema,
    component: ProgressRing
  },

  // Charts
  'line-chart': {
    name: 'Line Chart',
    category: 'charts',
    icon: 'LineChart',
    description: 'Time series line chart',
    configSchema: LineChartConfigSchema,
    component: LineChart
  },
  'bar-chart': {
    name: 'Bar Chart',
    category: 'charts',
    icon: 'BarChart',
    description: 'Vertical or horizontal bars',
    configSchema: BarChartConfigSchema,
    component: BarChart
  },
  'gauge-chart': {
    name: 'Gauge',
    category: 'charts',
    icon: 'Gauge',
    description: 'Gauge/meter display (0-100%)',
    configSchema: GaugeConfigSchema,
    component: GaugeChart
  },

  // Lists
  'data-table': {
    name: 'Data Table',
    category: 'lists',
    icon: 'Table',
    description: 'Sortable, filterable table',
    configSchema: DataTableConfigSchema,
    component: DataTable
  },
  'log-feed': {
    name: 'Log Feed',
    category: 'lists',
    icon: 'ScrollText',
    description: 'Scrolling log view',
    configSchema: LogFeedConfigSchema,
    component: LogFeed
  },

  // Controls
  'toggle-switch': {
    name: 'Toggle',
    category: 'controls',
    icon: 'ToggleLeft',
    description: 'On/off toggle switch',
    configSchema: ToggleConfigSchema,
    component: ToggleSwitch
  },
  'slider': {
    name: 'Slider',
    category: 'controls',
    icon: 'SlidersHorizontal',
    description: 'Numeric slider input',
    configSchema: SliderConfigSchema,
    component: Slider
  },

  // ... more generic components
}

// Business components (NeoMind specific)
export const BusinessComponentRegistry = {
  // AI Agents ⭐ (highlighted as core feature)
  'agent-status-card': {
    name: 'Agent Status Card',
    category: 'business.agents',
    icon: 'Bot',
    description: 'Agent overview with execute button',
    featured: true,  // Highlight as important
    component: AgentStatusCard,
    configSchema: { agentId: 'string' }
  },
  'agent-activity-list': {
    name: 'Agent Activity',
    category: 'business.agents',
    icon: 'Activity',
    description: 'Recent agent executions',
    featured: true,
    component: AgentActivityList,
    configSchema: { agentId: 'string', limit: 'number?' }
  },
  'agent-execution-chart': {
    name: 'Agent Execution Chart',
    category: 'business.agents',
    icon: 'LineChart',
    description: 'Agent execution timeline',
    component: AgentExecutionChart,
    configSchema: { agentId: 'string' }
  },
  'agent-memory-view': {
    name: 'Agent Memory',
    category: 'business.agents',
    icon: 'Brain',
    description: 'Learned patterns display',
    component: AgentMemoryView,
    configSchema: { agentId: 'string' }
  },
  'agent-chat': {
    name: 'Agent Chat',
    category: 'business.agents',
    icon: 'MessageSquare',
    description: 'Quick agent execution',
    featured: true,
    component: AgentChat,
    configSchema: { agentId: 'string' }
  },

  // Devices
  'device-grid': {
    name: 'Device Grid',
    category: 'business.devices',
    icon: 'Server',
    description: 'All devices status',
    component: DeviceGrid,
    configSchema: { filter: 'object?', sortBy: 'string?' }
  },
  'device-telemetry': {
    name: 'Device Telemetry',
    category: 'business.devices',
    icon: 'Gauge',
    description: 'Current device metrics',
    component: DeviceTelemetry,
    configSchema: { deviceId: 'string', metrics: 'string[]' }
  },
  'device-control': {
    name: 'Device Control',
    category: 'business.devices',
    icon: 'Settings',
    description: 'Toggle/slider for device control',
    component: DeviceControl,
    configSchema: { deviceId: 'string' }
  },

  // Rules
  'rule-status-grid': {
    name: 'Rule Status',
    category: 'business.rules',
    icon: 'Scroll',
    description: 'Rule enable/disable status',
    component: RuleStatusGrid,
    configSchema: { enabledOnly: 'boolean?' }
  },
  'rule-trigger-log': {
    name: 'Rule Triggers',
    category: 'business.rules',
    icon: 'Zap',
    description: 'Recent rule triggers',
    component: RuleTriggerLog,
    configSchema: { ruleId: 'string' }
  },

  // Alerts
  'alert-panel': {
    name: 'Alert Panel',
    category: 'business.alerts',
    icon: 'AlertTriangle',
    description: 'Active alerts with severity',
    component: AlertPanel,
    configSchema: {}
  },
  'alert-by-severity': {
    name: 'Alert Counts',
    category: 'business.alerts',
    icon: 'BadgeAlert',
    description: 'Count by severity',
    component: AlertBySeverity,
    configSchema: {}
  },

  // Decisions
  'decision-list': {
    name: 'Decision List',
    category: 'business.decisions',
    icon: 'GitBranch',
    description: 'Recent AI decisions',
    component: DecisionList,
    configSchema: {}
  },
}

// Combined registry for UI
export const AllComponents = {
  ...GenericComponentRegistry,  // Data-agnostic components
  ...BusinessComponentRegistry  // Pre-configured business components
}
```

---

## User Flow

### Creating a Dashboard

1. User navigates to `/dashboard`
2. Sees default "Overview" template
3. Clicks "New Dashboard" button
4. Selects a template or starts blank
5. Dashboard opens in edit mode

### Adding Components

1. Click "Add Component" button
2. Component library opens (grouped by category)
3. Select component type (e.g., Value Card, Line Chart)
4. Configure:
   - Select data source (device, agent, rule, etc.)
   - Set display options (format, colors, thresholds)
   - Add title
5. Component added to dashboard
6. Drag to position, resize as needed

### Configuring a Component

1. Click component gear icon
2. Configuration panel opens
3. Options based on component type:
   - **Data Source**: Choose API endpoint, set refresh rate
   - **Display**: Format, units, colors, thresholds
   - **Actions**: Button clicks, toggles, etc.
4. Save to apply changes

### Example: Adding a Temperature Display

1. Click "Add Component"
2. Select "Value Card" from Indicators
3. Configure:
   - Title: "Living Room Temp"
   - Data Source: Devices → sensor-001 → temperature
   - Format: "0.1" Unit: "°C"
   - Color Scale: Blue (<18) → Green (18-26) → Orange (>26)
4. Click "Add"
5. Drag to desired position

### Editing the Dashboard

1. Click "Edit Mode" toggle
2. Drag components to rearrange
3. Resize by dragging corners
4. Click gear to configure, X to delete
5. Click "Save" to persist

---

## Success Criteria

1. ✅ **Two-Layer Component System**: Generic components for flexibility + Business components for convenience
2. ✅ **Agent-First**: AI Agent components are featured and easy to use
3. ✅ All components use existing APIs - no backend changes required
4. ✅ Users can create a custom dashboard in < 2 minutes
5. ✅ All components update in real-time via WebSocket
6. ✅ Dashboard state persists across sessions
7. ✅ Mobile responsive (breakpoints work)
8. ✅ Performance: < 100ms to load dashboard

---

## Component Summary

### Generic IoT Components (20+)

| Category | Components | Data Agnostic |
|----------|------------|---------------|
| **Indicators** | value-card, status-badge, progress-ring, trend-value, sparkline | ✅ |
| **Charts** | line-chart, area-chart, bar-chart, gauge-chart, pie-chart, heatmap | ✅ |
| **Lists** | data-table, card-list, status-list, log-feed | ✅ |
| **Controls** | toggle-switch, slider, button-group, dropdown, input-field | ✅ |
| **Containers** | tabs, accordion, grid, divider | ✅ |
| **Notifications** | alert-banner, badge-counter, status-panel | ✅ |

### Business Components (15+)

| Category | Components | Pre-Configured |
|----------|------------|----------------|
| **🤖 AI Agents** ⭐ | agent-status-card, agent-activity-list, agent-execution-chart, agent-memory-view, agent-chat, agent-conversation | ✅ |
| **📱 Devices** | device-grid, device-telemetry, device-control, device-health | ✅ |
| **📜 Rules** | rule-status-grid, rule-trigger-log, rule-tester | ✅ |
| **🔄 Transforms** | transform-list, virtual-metrics | ✅ |
| **⚠️ Alerts** | alert-panel, alert-by-severity, alert-timeline | ✅ |
| **🧠 Decisions** | decision-list, decision-detail, decision-stats | ✅ |

---

## Key Advantages: Two-Layer Approach

| Aspect | Custom Widgets Only | Generic + Business Components |
|--------|-------------------|------------------------------|
| **Flexibility** | Fixed to specific data | Generic = any data, Business = pre-configured |
| **Setup Time** | High | Business = 1 click, Generic = 5 min |
| **Composability** | Limited | Mix both types freely |
| **Maintenance** | New widget per use case | Reuse generic, extend business |
| **Learning Curve** | Learn each widget | Learn generic, business is self-explanatory |
| **Agent Support** | Bolted on | **Featured components** |

### Example: Displaying Agent Execution Count

**Using Generic Component** (flexible but more config):
```json
{
  "type": "value-card",
  "dataSource": {
    "endpoint": "/agents/agent-001/stats",
    "transform": "data.total_executions"
  },
  "display": { "format": "#,###", "unit": "runs" }
}
```

**Using Business Component** (simple):
```json
{
  "type": "agent-status-card",
  "agentId": "agent-001"
}
```

### When to Use Each Layer

| Scenario | Recommended |
|----------|-------------|
| Quick system monitoring | **Business Components** - one click setup |
| Custom data visualization | **Generic Components** - full control |
| Agent monitoring | **Business Components** - specialized for agents |
| Mixed dashboards | **Both** - combine business + generic |
| Unique metrics | **Generic Components** - configure any data |

---

## Component Configuration UI

The configuration panel adapts based on component type:

### Generic Component Configuration (More Options)

```
┌─────────────────────────────────────────────┐
│ Configure: Value Card                        │
├─────────────────────────────────────────────┤
│ Title: [Living Room Temperature            ] │
│                                              │
│ Data Source:                                 │
│   Category: [Devices ▼]                       │
│   Source:   [sensor-001 ▼]                    │
│   Field:    [summary.temperature ▼]          │
│                                              │
│ Display:                                     │
│   Format:   [0.1        ] Unit: [°C      ]  │
│   Size:     ○ Small ● Large ○ XL             │
│                                              │
│ Color Scale: [+]                             │
│   < 18 → Blue                                │
│   18-26 → Green                              │
│   > 26 → Orange                              │
│                                              │
│ Refresh: [10] seconds                        │
│                                              │
│           [Cancel]  [Apply]                  │
└─────────────────────────────────────────────┘
```

### Business Component Configuration (Simplified)

```
┌─────────────────────────────────────────────┐
│ Configure: Agent Status Card ⭐              │
├─────────────────────────────────────────────┤
│ Select Agent:                                │
│                                              │
│   [Temperature Monitor ▼]                    │
│   • Status: Active                           │
│   • Last run: 5 minutes ago                  │
│   • Executions: 156                          │
│                                              │
│ Options:                                     │
│   ☑ Show execute button                      │
│   ☑ Show execution stats                     │
│   ☐ Show memory preview                      │
│                                              │
│           [Cancel]  [Add]                    │
└─────────────────────────────────────────────┘
```
