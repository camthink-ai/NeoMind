/**
 * Component Registry
 *
 * Centralized metadata registry for all dashboard components.
 * Provides component info for the component library and rendering.
 */

import type {
  ComponentRegistry,
  ComponentMeta,
  RegistryFilterOptions,
  GroupedComponentRegistry,
  ComponentCategory,
} from './types'
import type { ComponentType } from '@/types/dashboard'
import { COMPONENT_SIZE_CONSTRAINTS } from '@/types/dashboard'

// ============================================================================
// Icon Imports
// ============================================================================

import {
  // Indicators
  Hash,
  Circle,
  TrendingUp,
  Layers,
  // Charts
  LineChart as LineChartIcon,
  BarChart3,
  Gauge,
  PieChart as PieChartIcon,
  Donut,
  // Controls
  ToggleLeft,
  Layers as LayersIcon,
  List,
  Type,
  // Lists & Tables
  Table,
  ListTodo,
  Scroll,
  // Layout & Content
  Layers as LayoutIcon,
  Heading as HeadingIcon,
  AlertTriangle,
  // Business
  Bot,
  Brain,
  SlidersHorizontal,
  GitBranch,
  Workflow,
} from 'lucide-react'

// ============================================================================
// Component Metadata Definitions
// ============================================================================

// Helper to create size constraints with defaults
function getSizeConstraints(type: ComponentType) {
  return COMPONENT_SIZE_CONSTRAINTS[type] || {
    minW: 2,
    minH: 2,
    defaultW: 4,
    defaultH: 3,
    maxW: 12,
    maxH: 12,
  }
}

// All component metadata
export const componentRegistry: ComponentRegistry = {
  // ============================================================================
  // Indicators
  // ============================================================================

  'value-card': {
    type: 'value-card',
    name: 'Value Card',
    description: 'Display a single value with optional unit and trend',
    category: 'indicators',
    icon: Hash,
    sizeConstraints: getSizeConstraints('value-card'),
    hasDataSource: true,
    hasDisplayConfig: true,
    hasActions: false,
    acceptsProp: (prop) => [
      'label', 'unit', 'prefix', 'suffix', 'size', 'variant',
      'showTrend', 'trendValue', 'trendPeriod', 'showSparkline',
      'color', 'className'
    ].includes(prop),
    defaultProps: {
      size: 'md',
      variant: 'default',
      showTrend: false,
      showSparkline: false,
    },
    variants: ['default', 'vertical', 'compact'],
  },

  'led-indicator': {
    type: 'led-indicator',
    name: 'LED Indicator',
    description: 'Simple LED status indicator light',
    category: 'indicators',
    icon: Circle,
    sizeConstraints: getSizeConstraints('led-indicator'),
    hasDataSource: true,
    hasDisplayConfig: true,
    hasActions: false,
    acceptsProp: (prop) => [
      'label', 'color', 'size', 'variant', 'blink', 'className'
    ].includes(prop),
    defaultProps: {
      size: 'md',
      variant: 'default',
      blink: false,
    },
    variants: ['default', 'labeled'],
  },

  'sparkline': {
    type: 'sparkline',
    name: 'Sparkline',
    description: 'Mini trend chart showing data history',
    category: 'indicators',
    icon: TrendingUp,
    sizeConstraints: getSizeConstraints('sparkline'),
    hasDataSource: true,
    hasDisplayConfig: true,
    hasActions: false,
    acceptsProp: (prop) => [
      'data', 'color', 'showArea', 'showDots', 'smooth', 'className'
    ].includes(prop),
    defaultProps: {
      showArea: true,
      showDots: false,
      smooth: true,
    },
  },

  'progress-bar': {
    type: 'progress-bar',
    name: 'Progress Bar',
    description: 'Linear progress indicator',
    category: 'indicators',
    icon: Layers,
    sizeConstraints: getSizeConstraints('progress-bar'),
    hasDataSource: true,
    hasDisplayConfig: true,
    hasActions: false,
    acceptsProp: (prop) => [
      'value', 'min', 'max', 'color', 'showLabel', 'variant', 'className'
    ].includes(prop),
    defaultProps: {
      min: 0,
      max: 100,
      showLabel: true,
      variant: 'default',
    },
    variants: ['default', 'thin', 'thick'],
  },

  // ============================================================================
  // Charts
  // ============================================================================

  'line-chart': {
    type: 'line-chart',
    name: 'Line Chart',
    description: 'Time series line chart',
    category: 'charts',
    icon: LineChartIcon,
    sizeConstraints: getSizeConstraints('line-chart'),
    hasDataSource: true,
    hasDisplayConfig: true,
    hasActions: false,
    acceptsProp: (prop) => [
      'data', 'labels', 'colors', 'smooth', 'showGrid',
      'showLegend', 'showTooltip', 'fillArea', 'className'
    ].includes(prop),
    defaultProps: {
      smooth: true,
      showGrid: true,
      showLegend: false,
      showTooltip: true,
      fillArea: false,
    },
  },

  'area-chart': {
    type: 'area-chart',
    name: 'Area Chart',
    description: 'Area chart with filled region under line',
    category: 'charts',
    icon: LineChartIcon,
    sizeConstraints: getSizeConstraints('area-chart'),
    hasDataSource: true,
    hasDisplayConfig: true,
    hasActions: false,
    acceptsProp: (prop) => [
      'data', 'labels', 'colors', 'smooth', 'showGrid',
      'showLegend', 'showTooltip', 'opacity', 'className'
    ].includes(prop),
    defaultProps: {
      smooth: true,
      showGrid: true,
      showLegend: false,
      showTooltip: true,
      opacity: 0.3,
    },
  },

  'bar-chart': {
    type: 'bar-chart',
    name: 'Bar Chart',
    description: 'Vertical bar chart for categorical data',
    category: 'charts',
    icon: BarChart3,
    sizeConstraints: getSizeConstraints('bar-chart'),
    hasDataSource: true,
    hasDisplayConfig: true,
    hasActions: false,
    acceptsProp: (prop) => [
      'data', 'labels', 'colors', 'horizontal', 'showGrid',
      'showLegend', 'showTooltip', 'className'
    ].includes(prop),
    defaultProps: {
      horizontal: false,
      showGrid: true,
      showLegend: false,
      showTooltip: true,
    },
    variants: ['vertical', 'horizontal', 'stacked'],
  },

  'pie-chart': {
    type: 'pie-chart',
    name: 'Pie Chart',
    description: 'Pie chart for part-to-whole relationships',
    category: 'charts',
    icon: PieChartIcon,
    sizeConstraints: getSizeConstraints('pie-chart'),
    hasDataSource: true,
    hasDisplayConfig: true,
    hasActions: false,
    acceptsProp: (prop) => [
      'data', 'colors', 'showLabels', 'showLegend', 'showTooltip', 'innerRadius', 'className'
    ].includes(prop),
    defaultProps: {
      showLabels: false,
      showLegend: false,
      showTooltip: true,
    },
  },

  'donut-chart': {
    type: 'donut-chart',
    name: 'Donut Chart',
    description: 'Hollow pie chart (donut variant)',
    category: 'charts',
    icon: Donut,
    sizeConstraints: getSizeConstraints('donut-chart'),
    hasDataSource: true,
    hasDisplayConfig: true,
    hasActions: false,
    acceptsProp: (prop) => [
      'data', 'colors', 'showLabels', 'showLegend', 'showTooltip', 'innerRadius', 'className'
    ].includes(prop),
    defaultProps: {
      showLabels: false,
      showLegend: false,
      showTooltip: true,
      innerRadius: '60%',
    },
  },

  'gauge-chart': {
    type: 'gauge-chart',
    name: 'Gauge Chart',
    description: 'Semi-circle gauge for values in a range',
    category: 'charts',
    icon: Gauge,
    sizeConstraints: getSizeConstraints('gauge-chart'),
    hasDataSource: true,
    hasDisplayConfig: true,
    hasActions: false,
    acceptsProp: (prop) => [
      'value', 'min', 'max', 'label', 'unit', 'zones',
      'showValue', 'variant', 'className'
    ].includes(prop),
    defaultProps: {
      min: 0,
      max: 100,
      showValue: true,
      variant: 'gauge',
    },
    variants: ['gauge', 'semi', 'arc'],
  },

  // ============================================================================
  // Controls
  // ============================================================================

  'toggle-switch': {
    type: 'toggle-switch',
    name: 'Toggle Switch',
    description: 'On/off toggle switch',
    category: 'controls',
    icon: ToggleLeft,
    sizeConstraints: getSizeConstraints('toggle-switch'),
    hasDataSource: true,
    hasDisplayConfig: false,
    hasActions: true,
    acceptsProp: (prop) => [
      'label', 'description', 'size', 'variant',
      'trueLabel', 'falseLabel', 'trueIcon', 'falseIcon',
      'disabled', 'showCard', 'className'
    ].includes(prop),
    defaultProps: {
      size: 'md',
      variant: 'default',
      showCard: true,
      trueLabel: 'On',
      falseLabel: 'Off',
    },
    variants: ['default', 'icon', 'slider', 'pill'],
  },

  'button-group': {
    type: 'button-group',
    name: 'Button Group',
    description: 'Group of action buttons',
    category: 'controls',
    icon: LayersIcon,
    sizeConstraints: getSizeConstraints('button-group'),
    hasDataSource: false,
    hasDisplayConfig: false,
    hasActions: true,
    acceptsProp: (prop) => [
      'buttons', 'size', 'variant', 'orientation', 'className'
    ].includes(prop),
    defaultProps: {
      size: 'md',
      variant: 'default',
      orientation: 'horizontal',
    },
    variants: ['default', 'segmented', 'icon-only'],
  },

  'dropdown': {
    type: 'dropdown',
    name: 'Dropdown',
    description: 'Select dropdown',
    category: 'controls',
    icon: List,
    sizeConstraints: getSizeConstraints('dropdown'),
    hasDataSource: true,
    hasDisplayConfig: false,
    hasActions: true,
    acceptsProp: (prop) => [
      'label', 'options', 'placeholder', 'size', 'disabled', 'className'
    ].includes(prop),
    defaultProps: {
      size: 'md',
      placeholder: 'Select...',
    },
  },

  'input-field': {
    type: 'input-field',
    name: 'Input Field',
    description: 'Text input field',
    category: 'controls',
    icon: Type,
    sizeConstraints: getSizeConstraints('input-field'),
    hasDataSource: true,
    hasDisplayConfig: false,
    hasActions: true,
    acceptsProp: (prop) => [
      'label', 'placeholder', 'type', 'size', 'disabled', 'className'
    ].includes(prop),
    defaultProps: {
      size: 'md',
      type: 'text',
    },
  },

  // ============================================================================
  // Lists & Tables
  // ============================================================================

  'data-table': {
    type: 'data-table',
    name: 'Data Table',
    description: 'Sortable table with data rows',
    category: 'lists',
    icon: Table,
    sizeConstraints: getSizeConstraints('data-table'),
    hasDataSource: true,
    hasDisplayConfig: true,
    hasActions: false,
    acceptsProp: (prop) => [
      'columns', 'data', 'sortable', 'pageSize', 'showHeader', 'className'
    ].includes(prop),
    defaultProps: {
      sortable: true,
      pageSize: 10,
      showHeader: true,
    },
  },

  'status-list': {
    type: 'status-list',
    name: 'Status List',
    description: 'List of status items with indicators',
    category: 'lists',
    icon: ListTodo,
    sizeConstraints: getSizeConstraints('status-list'),
    hasDataSource: true,
    hasDisplayConfig: true,
    hasActions: false,
    acceptsProp: (prop) => [
      'items', 'showIcon', 'showTimestamp', 'compact', 'className'
    ].includes(prop),
    defaultProps: {
      showIcon: true,
      showTimestamp: true,
      compact: false,
    },
  },

  'log-feed': {
    type: 'log-feed',
    name: 'Log Feed',
    description: 'Scrolling log display',
    category: 'lists',
    icon: Scroll,
    sizeConstraints: getSizeConstraints('log-feed'),
    hasDataSource: true,
    hasDisplayConfig: true,
    hasActions: false,
    acceptsProp: (prop) => [
      'maxLines', 'autoScroll', 'showTimestamp', 'showLevel', 'className'
    ].includes(prop),
    defaultProps: {
      maxLines: 50,
      autoScroll: true,
      showTimestamp: true,
      showLevel: true,
    },
  },

  // ============================================================================
  // ============================================================================
  // Business Components
  // ============================================================================

  'agent-status-card': {
    type: 'agent-status-card',
    name: 'Agent Status',
    description: 'AI agent status and execution stats',
    category: 'business',
    icon: Bot,
    sizeConstraints: getSizeConstraints('agent-status-card'),
    hasDataSource: true,
    hasDisplayConfig: false,
    hasActions: true,
    acceptsProp: (prop) => [
      'agentId', 'name', 'description', 'showStats', 'showExecuteButton', 'size', 'className'
    ].includes(prop),
    defaultProps: {
      size: 'md',
      showStats: true,
      showExecuteButton: true,
    },
  },

  'decision-list': {
    type: 'decision-list',
    name: 'Decision List',
    description: 'List of AI decisions with actions',
    category: 'business',
    icon: Brain,
    sizeConstraints: getSizeConstraints('decision-list'),
    hasDataSource: true,
    hasDisplayConfig: false,
    hasActions: true,
    acceptsProp: (prop) => [
      'agentId', 'maxItems', 'showActions', 'compact', 'className'
    ].includes(prop),
    defaultProps: {
      maxItems: 10,
      showActions: true,
      compact: false,
    },
  },

  'device-control': {
    type: 'device-control',
    name: 'Device Control',
    description: 'Device control interface',
    category: 'business',
    icon: SlidersHorizontal,
    sizeConstraints: getSizeConstraints('device-control'),
    hasDataSource: true,
    hasDisplayConfig: false,
    hasActions: true,
    acceptsProp: (prop) => [
      'deviceId', 'controls', 'layout', 'className'
    ].includes(prop),
    defaultProps: {
      layout: 'default',
    },
    variants: ['default', 'compact', 'detailed'],
  },

  'rule-status-grid': {
    type: 'rule-status-grid',
    name: 'Rule Status Grid',
    description: 'Grid showing rule statuses',
    category: 'business',
    icon: GitBranch,
    sizeConstraints: getSizeConstraints('rule-status-grid'),
    hasDataSource: true,
    hasDisplayConfig: false,
    hasActions: true,
    acceptsProp: (prop) => [
      'rules', 'groupBy', 'showStats', 'className'
    ].includes(prop),
    defaultProps: {
      groupBy: 'status',
      showStats: true,
    },
  },

  'transform-list': {
    type: 'transform-list',
    name: 'Transform List',
    description: 'Data transform configurations list',
    category: 'business',
    icon: Workflow,
    sizeConstraints: getSizeConstraints('transform-list'),
    hasDataSource: true,
    hasDisplayConfig: false,
    hasActions: true,
    acceptsProp: (prop) => [
      'transforms', 'showStats', 'compact', 'className'
    ].includes(prop),
    defaultProps: {
      showStats: true,
      compact: false,
    },
  },
} as const

// ============================================================================
// Registry Helpers
// ============================================================================

/**
 * Get component metadata by type
 */
export function getComponentMeta(type: ComponentType): ComponentMeta | undefined {
  return componentRegistry[type]
}

/**
 * Get all component types
 */
export function getAllComponentTypes(): ComponentType[] {
  return Object.keys(componentRegistry) as ComponentType[]
}

/**
 * Get all component metadata as array
 */
export function getAllComponents(): ComponentMeta[] {
  return Object.values(componentRegistry)
}

/**
 * Filter components by options
 */
export function filterComponents(options: RegistryFilterOptions = {}): ComponentMeta[] {
  let components = Object.values(componentRegistry)

  if (options.category) {
    components = components.filter(c => c.category === options.category)
  }

  if (options.hasDataSource !== undefined) {
    components = components.filter(c => c.hasDataSource === options.hasDataSource)
  }

  if (options.searchQuery) {
    const query = options.searchQuery.toLowerCase()
    components = components.filter(c =>
      c.name.toLowerCase().includes(query) ||
      c.description.toLowerCase().includes(query) ||
      c.type.includes(query)
    )
  }

  return components
}

/**
 * Group components by category
 */
export function groupComponentsByCategory(options: RegistryFilterOptions = {}): GroupedComponentRegistry {
  const components = filterComponents(options)

  const grouped = components.reduce((acc, component) => {
    const category = component.category
    if (!acc[category]) {
      acc[category] = {
        category,
        components: [],
      }
    }
    acc[category].components.push(component)
    return acc
  }, {} as Record<ComponentCategory, GroupedComponentRegistry[number]>)

  // Return in a consistent order
  const categoryOrder: ComponentCategory[] = [
    'indicators', 'charts', 'controls', 'lists', 'layout', 'business'
  ]

  return categoryOrder
    .filter(cat => grouped[cat])
    .map(cat => grouped[cat])
}

/**
 * Get category info
 */
export function getCategoryInfo(category: ComponentCategory): { name: string; icon: React.ComponentType<{ className?: string }> } {
  const categoryInfos: Record<ComponentCategory, { name: string; icon: React.ComponentType<{ className?: string }> }> = {
    indicators: { name: 'Indicators', icon: Hash },
    charts: { name: 'Charts', icon: LineChartIcon },
    controls: { name: 'Controls', icon: ToggleLeft },
    lists: { name: 'Lists & Tables', icon: Table },
    layout: { name: 'Layout & Content', icon: LayoutIcon },
    business: { name: 'Business', icon: Bot },
  }

  return categoryInfos[category]
}
