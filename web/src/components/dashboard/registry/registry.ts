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
  Send,
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
  // Business
  Camera,
  ScanEye,
  // Community
  Store,
  Code,
  // Fallback for extension-declared categories
  Puzzle,
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
      'valueMap', 'defaultState', 'showGlow', 'showAnimation',
      'dataMapping', 'showCard', 'rules',
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
      'limit', 'timeRange', 'aggregate', 'dataMapping'
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
      'limit', 'timeRange', 'aggregate', 'dataMapping'
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
    name: 'Command Button',
    description: 'Trigger button that opens a command form dialog for device or extension commands',
    category: 'controls',
    icon: Send,
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
    variants: ['file', 'stream', 'rtsp', 'hls', 'webrtc', 'camera'],
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

  'ai-analyst': {
    type: 'ai-analyst',
    name: 'AI Analyst',
    description: 'AI-powered data analysis — auto-analyze images, metrics, and data sources in a timeline chat',
    category: 'business',
    icon: ScanEye,
    sizeConstraints: getSizeConstraints('ai-analyst'),
    hasDataSource: true,
    hasDisplayConfig: false,
    hasActions: true,
    acceptsProp: (prop) => ['agentId', 'sessionId', 'className', 'editMode', 'modelId', 'modelName', 'systemPrompt', 'contextWindowSize', 'title'].includes(prop),
    defaultProps: { agentId: undefined },
    variants: ['default'],
  },
} as const

// ============================================================================
// Registry Helpers
// ============================================================================

// Import dynamic registries
import { dynamicRegistry, dtoToComponentMeta } from './DynamicRegistry'
import { communityRegistry } from './CommunityRegistry'

/**
 * Get component metadata by type
 * Checks static (built-in), dynamic (extension-provided), and community registries
 */
export function getComponentMeta(type: ComponentType): ComponentMeta | undefined {
  // First try static registry
  const staticMeta = componentRegistry[type]
  if (staticMeta) return staticMeta

  // Then try dynamic registry (extensions)
  const dto = dynamicRegistry.getMeta(type)
  if (dto) {
    return dtoToComponentMeta(dto)
  }

  // Finally try community registry (marketplace components)
  const communityMeta = communityRegistry.getMeta(type)
  if (communityMeta) {
    return communityRegistry.communityMetaToComponentMeta(communityMeta)
  }

  return undefined
}

/**
 * Get all component types (static + dynamic + community)
 */
export function getAllComponentTypes(): ComponentType[] {
  const staticTypes = Object.keys(componentRegistry) as ComponentType[]
  const dynamicDtos = dynamicRegistry.getAllMetas()
  const dynamicTypes = dynamicDtos.map(dto => dto.type) as ComponentType[]
  const communityMetas = communityRegistry.getAllMetas()
  const communityTypes = communityMetas.map(meta => meta.id) as ComponentType[]

  return [...staticTypes, ...dynamicTypes, ...communityTypes]
}

/**
 * Get all component metadata as array (static + dynamic + community)
 */
export function getAllComponents(): ComponentMeta[] {
  const staticComponents = Object.values(componentRegistry)
  const dynamicDtos = dynamicRegistry.getAllMetas()
  const dynamicComponents = dynamicDtos.map(dto => dtoToComponentMeta(dto))
  const communityMetas = communityRegistry.getAllMetas()
  const communityComponents = communityMetas.map(meta => communityRegistry.communityMetaToComponentMeta(meta))

  return [...staticComponents, ...dynamicComponents, ...communityComponents]
}

/**
 * Filter components by options (includes dynamic and community components)
 */
export function filterComponents(options: RegistryFilterOptions = {}): ComponentMeta[] {
  const staticComponents = Object.values(componentRegistry)
  const dynamicDtos = dynamicRegistry.getAllMetas()
  const dynamicComponents = dynamicDtos.map(dto => dtoToComponentMeta(dto))
  const communityMetas = communityRegistry.getAllMetas()
  const communityComponents = communityMetas.map(meta => communityRegistry.communityMetaToComponentMeta(meta))

  let components = [...staticComponents, ...dynamicComponents, ...communityComponents]

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
 * Group components by category (includes dynamic and community components)
 *
 * Community components are split into "local" (CLI/upload) and "marketplace"
 * based on the `source` field — handled by communityMetaToComponentMeta().
 */
export function groupComponentsByCategory(options: RegistryFilterOptions = {}): GroupedComponentRegistry {
  const components = filterComponents(options)

  const grouped = components.reduce((acc, component) => {
    // Defensive: a missing/null category lands in the canonical
    // extension group rather than a "null" bucket.
    const category = component.category || 'custom'
    if (!acc[category]) {
      acc[category] = {
        category,
        components: [],
      }
    }
    acc[category].components.push(component)
    return acc
  }, {} as Record<string, GroupedComponentRegistry[number]>)

  // Built-in categories render in a curated order
  const categoryOrder: ComponentCategory[] = [
    'indicators',
    'charts',
    'controls',
    'display',
    'spatial',
    'business',
    'custom', // Extension components
    'local', // Local/AI-created community components
    'marketplace', // Marketplace-installed community components
  ]

  // Any other category (extension-declared, e.g. "fitness") is still a
  // valid group — render it after the built-ins instead of dropping it.
  const known = categoryOrder.filter(cat => grouped[cat]).map(cat => grouped[cat])
  const unknown = Object.keys(grouped)
    .filter(cat => !categoryOrder.includes(cat as ComponentCategory))
    .sort((a, b) => a.localeCompare(b))
    .map(cat => grouped[cat])

  return [...known, ...unknown]
}

/**
 * Humanize a raw category id ("my-widgets" → "My Widgets") for
 * extension-declared categories that have no curated metadata.
 */
function prettifyCategory(category: string): string {
  return category
    .replace(/[-_]+/g, ' ')
    .trim()
    .replace(/\b[a-z]/g, ch => ch.toUpperCase())
}

/**
 * Get category info
 *
 * Unknown (extension-declared) categories fall back to a prettified
 * label and a generic icon so they render like any other group.
 */
export function getCategoryInfo(category: ComponentCategory): { name: string; icon: React.ComponentType<{ className?: string }> } {
  const categoryInfos: Record<string, { name: string; icon: React.ComponentType<{ className?: string }> }> = {
    indicators: { name: 'Indicators', icon: Hash },
    charts: { name: 'Charts', icon: LineChartIcon },
    controls: { name: 'Controls', icon: Send },
    display: { name: 'Display & Content', icon: Image },
    spatial: { name: 'Spatial & Media', icon: MapPin },
    business: { name: 'Business Components', icon: Bot },
    custom: { name: 'Extension Components', icon: LayersIcon }, // For dynamic components
    local: { name: 'My Components', icon: Code }, // Local/AI-created community components
    marketplace: { name: 'Marketplace', icon: Store }, // Marketplace-installed community components
  }

  return categoryInfos[category] ?? { name: prettifyCategory(String(category)), icon: Puzzle }
}
