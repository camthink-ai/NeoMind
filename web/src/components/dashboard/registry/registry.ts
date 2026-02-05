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
  Bot,
  // Charts
  LineChart as LineChartIcon,
  BarChart3,
  PieChart as PieChartIcon,
  // Controls
  ToggleLeft,
  Layers as LayersIcon,
  Sliders as SliderIcon,
  List,
  Type,
  // Display & Content
  Image,
  Play,
  Globe,
  FileText,
  // Spatial & Media
  MapPin,
  Map,
  Webcam,
  Square as SquareIcon,
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
      'title', 'unit', 'prefix', 'suffix', 'size', 'variant',
      'showTrend',
      'icon', 'iconType', 'iconColor', 'valueColor',
      'description',
      'dataMapping', 'className'
    ].includes(prop),
    defaultProps: {
      size: 'md',
      variant: 'default',
      showTrend: false,
    },
    variants: ['default', 'vertical', 'compact', 'minimal'],
  },

  'led-indicator': {
    type: 'led-indicator',
    name: 'LED Indicator',
    description: 'Simple LED status indicator light with value-to-state mapping',
    category: 'indicators',
    icon: Circle,
    sizeConstraints: getSizeConstraints('led-indicator'),
    hasDataSource: true,
    hasDisplayConfig: true,
    hasActions: false,
    acceptsProp: (prop) => [
      'title', 'color', 'size', 'variant', 'className',
      'valueMap', 'defaultState', 'showGlow', 'showAnimation', 'state',
      'dataMapping', 'showCard',
    ].includes(prop),
    defaultProps: {
      size: 'md',
      variant: 'default',
      defaultState: 'unknown',
      showGlow: true,
      showAnimation: true,
      valueMap: [],
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
      'data', 'color', 'colorMode', 'fill', 'fillColor', 'showPoints', 'strokeWidth', 'curved',
      'showThreshold', 'threshold', 'thresholdColor', 'maxValue',
      'showValue', 'title', 'size', 'responsive', 'showCard',
      'dataMapping', 'className'
    ].includes(prop),
    defaultProps: {
      fill: true,
      showPoints: false,
      curved: true,
    },
  },

  'progress-bar': {
    type: 'progress-bar',
    name: 'Progress Bar',
    description: 'Linear progress indicator with data mapping support',
    category: 'indicators',
    icon: Layers,
    sizeConstraints: getSizeConstraints('progress-bar'),
    hasDataSource: true,
    hasDisplayConfig: true,
    hasActions: false,
    acceptsProp: (prop) => [
      'value', 'max', 'color', 'showCard', 'variant', 'title', 'size',
      'warningThreshold', 'dangerThreshold',
      'dataMapping', 'dataSource', 'className'
    ].includes(prop),
    defaultProps: {
      max: 100,
      variant: 'default',
      warningThreshold: 70,
      dangerThreshold: 90,
    },
    variants: ['default', 'compact', 'circular'],
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
    maxDataSources: 5,
    hasDisplayConfig: true,
    hasActions: false,
    acceptsProp: (prop) => [
      'data', 'labels', 'colors', 'smooth', 'showGrid',
      'showLegend', 'showTooltip', 'fillArea', 'className',
      'limit', 'timeRange', 'aggregate', 'chartViewMode', 'dataMapping'
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
    maxDataSources: 5,
    hasDisplayConfig: true,
    hasActions: false,
    acceptsProp: (prop) => [
      'data', 'labels', 'colors', 'smooth', 'showGrid',
      'showLegend', 'showTooltip', 'opacity', 'className',
      'limit', 'timeRange', 'aggregate', 'chartViewMode', 'dataMapping'
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
    maxDataSources: 3,
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
    maxDataSources: 1,  // Pie chart shows part-to-whole, single source makes more sense
    hasDisplayConfig: true,
    hasActions: false,
    acceptsProp: (prop) => [
      'data', 'colors', 'showLabels', 'showLegend', 'showTooltip', 'innerRadius', 'variant', 'className'
    ].includes(prop),
    defaultProps: {
      variant: 'donut',
      showLabels: false,
      showLegend: false,
      showTooltip: true,
    },
  },

  // ============================================================================
  // Controls
  // ============================================================================

  'toggle-switch': {
    type: 'toggle-switch',
    name: 'Toggle Switch',
    description: 'On/off toggle switch for device commands',
    category: 'controls',
    icon: ToggleLeft,
    sizeConstraints: getSizeConstraints('toggle-switch'),
    hasDataSource: true,
    hasDisplayConfig: false,
    hasActions: true,
    acceptsProp: (prop) => [
      'dataSource', 'title', 'label', 'size',
      'initialState', 'editMode', 'disabled', 'className'
    ].includes(prop),
    defaultProps: {
      size: 'md',
      initialState: false,
      disabled: false,
    },
    variants: ['default'],
  },

  // ============================================================================
  // Display & Content
  // ============================================================================

  'image-display': {
    type: 'image-display',
    name: 'Image Display',
    description: 'Display images from URLs or data sources',
    category: 'display',
    icon: Image,
    sizeConstraints: getSizeConstraints('image-display'),
    hasDataSource: true,
    hasDisplayConfig: true,
    hasActions: false,
    acceptsProp: (prop) => [
      'src', 'alt', 'caption', 'fit', 'objectPosition', 'rounded',
      'showShadow', 'zoomable', 'downloadable', 'size', 'className'
    ].includes(prop),
    defaultProps: {
      fit: 'contain',
      rounded: true,
      zoomable: true,
    },
    variants: ['contain', 'cover', 'fill'],
  },

  'image-history': {
    type: 'image-history',
    name: 'Image History',
    description: 'Display historical images with floating slider navigation',
    category: 'display',
    icon: Play,
    sizeConstraints: getSizeConstraints('image-history'),
    hasDataSource: true,
    hasDisplayConfig: true,
    hasActions: false,
    acceptsProp: (prop) => [
      'images', 'title', 'fit', 'rounded', 'showTimestamp', 'showLabel',
      'showIndex', 'size', 'className'
    ].includes(prop),
    defaultProps: {
      fit: 'cover',
      rounded: false,
      showIndex: true,
      showTimestamp: true,
      showLabel: false,
    },
  },

  'web-display': {
    type: 'web-display',
    name: 'Web Display',
    description: 'Display web content via iframe',
    category: 'display',
    icon: Globe,
    sizeConstraints: getSizeConstraints('web-display'),
    hasDataSource: true,
    hasDisplayConfig: true,
    hasActions: false,
    acceptsProp: (prop) => [
      'src', 'title', 'sandbox', 'allowFullscreen', 'allowScripts',
      'allowSameOrigin', 'allowForms', 'allowPopups', 'showHeader',
      'showUrlBar', 'transparent', 'borderless', 'size', 'className'
    ].includes(prop),
    defaultProps: {
      sandbox: true,
      allowFullscreen: true,
      showHeader: true,
    },
  },

  'markdown-display': {
    type: 'markdown-display',
    name: 'Markdown Display',
    description: 'Render markdown content with formatting',
    category: 'display',
    icon: FileText,
    sizeConstraints: getSizeConstraints('markdown-display'),
    hasDataSource: true,
    hasDisplayConfig: true,
    hasActions: false,
    acceptsProp: (prop) => [
      'content', 'variant', 'showCodeSyntax', 'allowHtml',
      'lineBreaks', 'maxLines', 'size', 'className'
    ].includes(prop),
    defaultProps: {
      variant: 'default',
      lineBreaks: true,
    },
    variants: ['default', 'compact', 'minimal'],
  },

  // ============================================================================
  // Spatial & Media
  // ============================================================================

  'map-display': {
    type: 'map-display',
    name: 'Map Display',
    description: 'Interactive map with device markers, metrics, and commands',
    category: 'spatial',
    icon: Map,
    sizeConstraints: getSizeConstraints('map-display'),
    hasDataSource: true,
    maxDataSources: 10,
    hasDisplayConfig: true,
    hasActions: true,
    acceptsProp: (prop) => [
      'markers', 'center', 'zoom', 'minZoom', 'maxZoom',
      'showControls', 'showFullscreen', 'interactive',
      'tileLayer', 'markerColor', 'size', 'className'
    ].includes(prop),
    defaultProps: {
      center: { lat: 39.9042, lng: 116.4074 },
      zoom: 10,
      showControls: true,
      interactive: true,
    },
    variants: ['default', 'satellite', 'dark', 'terrain'],
  },

  'video-display': {
    type: 'video-display',
    name: 'Video Display',
    description: 'Video player for streams and camera feeds',
    category: 'spatial',
    icon: Webcam,
    sizeConstraints: getSizeConstraints('video-display'),
    hasDataSource: true,
    hasDisplayConfig: true,
    hasActions: false,
    acceptsProp: (prop) => [
      'src', 'type', 'autoplay', 'muted', 'controls', 'loop', 'fit',
      'refreshInterval', 'reconnectAttempts', 'rounded', 'showFullscreen',
      'size', 'className'
    ].includes(prop),
    defaultProps: {
      type: 'file',
      autoplay: false,
      muted: true,
      controls: true,
      fit: 'contain',
    },
    variants: ['file', 'stream', 'rtsp', 'hls', 'camera'],
  },

  'custom-layer': {
    type: 'custom-layer',
    name: 'Custom Layer',
    description: 'Free-form container for devices, metrics, and commands',
    category: 'spatial',
    icon: SquareIcon,
    sizeConstraints: getSizeConstraints('custom-layer'),
    hasDataSource: true,
    maxDataSources: 20,
    hasDisplayConfig: true,
    hasActions: true,
    acceptsProp: (prop) => [
      'items', 'backgroundType', 'backgroundColor', 'backgroundImage', 'gridSize',
      'interactive', 'showControls', 'editable', 'showFullscreen',
      'maintainAspectRatio', 'aspectRatio', 'size', 'className'
    ].includes(prop),
    defaultProps: {
      backgroundType: 'grid',
      interactive: true,
      showControls: true,
      editable: false,
    },
    variants: ['grid', 'color', 'image', 'transparent'],
  },

  // ============================================================================
  // Business Components (placeholders for backward compatibility)
  // ============================================================================

  'agent-status-card': {
    type: 'agent-status-card',
    name: 'Agent Status Card',
    description: 'AI agent status and activity monitor with real-time execution tracking',
    category: 'indicators',
    icon: Bot,
    sizeConstraints: getSizeConstraints('agent-status-card'),
    hasDataSource: true,
    hasDisplayConfig: true,
    hasActions: true,
    acceptsProp: (prop) => [
      'agentId', 'agentName', 'title', 'description',
      'showExecutions', 'showSparkline', 'sparklineData', 'compact',
      'onExecute', 'onViewDetails', 'dataSource', 'className'
    ].includes(prop),
    defaultProps: {
      showExecutions: true,
      showSparkline: false,
      compact: false,
    },
    variants: ['default', 'compact'],
  },

  // ============================================================================
  // Business Components
  // ============================================================================

  'agent-monitor-widget': {
    type: 'agent-monitor-widget',
    name: 'Agent Monitor Widget',
    description: 'Comprehensive AI agent monitoring widget with agent selector, real-time status, statistics, and execution history',
    category: 'business',
    icon: Bot,
    sizeConstraints: getSizeConstraints('agent-monitor-widget'),
    hasDataSource: true,  // For balanced two-panel layout
    hasDisplayConfig: true,  // Agent selector in Display tab
    hasActions: true,
    acceptsProp: (prop) => [
      'agentId', 'className', 'editMode'
    ].includes(prop),
    defaultProps: {
      agentId: undefined,
    },
    variants: ['default'],
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
  }, {} as Record<string, GroupedComponentRegistry[number]>)

  // Return in a consistent order (without removed categories)
  const categoryOrder: ComponentCategory[] = [
    'indicators',
    'charts',
    'controls',
    'display',
    'spatial',
    'business',
  ]

  return categoryOrder
    .filter(cat => grouped[cat])
    .map(cat => grouped[cat])
}

/**
 * Get category info
 */
export function getCategoryInfo(category: ComponentCategory): { name: string; icon: React.ComponentType<{ className?: string }> } {
  const categoryInfos: Record<string, { name: string; icon: React.ComponentType<{ className?: string }> }> = {
    indicators: { name: 'Indicators', icon: Hash },
    charts: { name: 'Charts', icon: LineChartIcon },
    controls: { name: 'Controls', icon: ToggleLeft },
    display: { name: 'Display & Content', icon: Image },
    spatial: { name: 'Spatial & Media', icon: MapPin },
    business: { name: 'Business Components', icon: Bot },
  }

  return categoryInfos[category]
}
