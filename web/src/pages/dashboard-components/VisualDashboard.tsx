/**
 * Visual Dashboard Page
 *
 * Main dashboard page with grid layout, drag-and-drop, and component library.
 * Supports both generic IoT components and business components.
 */

import { useEffect, useState, useCallback, useRef, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { useStore } from '@/store'
import { cn } from '@/lib/utils'
import {
  LayoutDashboard,
  Plus,
  Check,
  Settings2,
  PanelsTopLeft,
  Copy,
  Trash2,
  Settings as SettingsIcon,
  ChevronRight,
  MoreVertical,
  // Indicator icons
  Hash,
  Circle,
  TrendingUp,
  Timer as TimerIcon,
  Hourglass,
  // Chart icons
  LineChart as LineChartIcon,
  BarChart3,
  PieChart as PieChartIcon,
  ScatterChart as ScatterChartIcon,
  Radar as RadarIcon,
  Filter,
  CandlestickChart,
  // Control icons
  ToggleLeft,
  Sliders as SliderIcon,
  RadioIcon,
  CheckSquare,
  ToggleLeft as SwitchIcon,
  Star,
  MapPin,
  Monitor,
  RotateCw,
  // Media icons
  Image as ImageIcon,
  Video as VideoIcon,
  Camera,
  Music,
  Globe,
  QrCode,
  Square as SquareIcon,
  Type,
  Code,
  Link,
  // Layout icons
  Layers,
  Container as ContainerIcon,
  MinusSquare,
  Grid,
  Minus,
  // Visualization icons
  Calendar as CalendarIcon,
  GitBranch,
  Network,
  Map as MapIcon,
  Zap,
  Box,
  Cloud,
  Sparkles,
  // Agent icons
  Bot,
  ListTodo,
  Clock,
  Brain,
  // Device icons
  Workflow,
  Activity,
  SlidersHorizontal,
  HeartPulse,
  // More icons
  FileText,
  Table,
  List,
  Scroll,
  Play,
} from 'lucide-react'
import { useParams, useNavigate } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { Field } from '@/components/ui/field'
import { Label } from '@/components/ui/label'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Sheet,
  SheetContent,
  SheetTitle,
  SheetTrigger,
} from '@/components/ui/sheet'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Checkbox } from '@/components/ui/checkbox'

// Config system
import {
  createDataDisplayConfig,
  createProgressConfig,
  createControlConfig,
  createIndicatorConfig,
  createContentConfig,
  createChartConfig,
  ComponentConfigDialog,
} from '@/components/dashboard/config'
import type { ComponentConfigSchema } from '@/components/dashboard/config/ComponentConfigBuilder'
import { ValueMapEditor } from '@/components/dashboard/config/ValueMapEditor'
import { DataMappingConfig } from '@/components/dashboard/config/UIConfigSections'
import type { ValueStateMapping } from '@/components/dashboard/config/ValueMapEditor'
import type { SingleValueMappingConfig, TimeSeriesMappingConfig, CategoricalMappingConfig } from '@/lib/dataMapping'

// UI components
import { ColorPicker } from '@/components/ui/color-picker'
import { EntityIconPicker } from '@/components/ui/entity-icon-picker'

// Dashboard components
import {
  DashboardGrid,
  // Indicators
  ValueCard,
  LEDIndicator,
  Sparkline,
  ProgressBar,
  // Charts
  LineChart,
  AreaChart,
  BarChart,
  PieChart,
  // Controls
  ToggleSwitch,
  // Display & Content
  ImageDisplay,
  ImageHistory,
  WebDisplay,
  MarkdownDisplay,
  // Spatial & Media
  MapDisplay,
  VideoDisplay,
  CustomLayer,
  LayerEditorDialog,
  MapEditorDialog,
  type MapBinding,
  type MapBindingType,
  type MapMarker,
  type LayerBinding,
  type LayerBindingType,
} from '@/components/dashboard'
import { DashboardListSidebar } from '@/components/dashboard/DashboardListSidebar'
import type { DashboardComponent, DataSourceOrList, DataSource, GenericComponent } from '@/types/dashboard'
import type { Device } from '@/types'
import { COMPONENT_SIZE_CONSTRAINTS } from '@/types/dashboard'
import { confirm } from '@/hooks/use-confirm'

// ============================================================================
// Helper Functions
// ============================================================================

/**
 * Memoized cache for converted telemetry data sources
 * Caches both individual DataSource objects AND complete arrays to prevent reference changes
 */
const telemetryCache: Record<string, any> = {}

/**
 * Convert device data source to telemetry with caching to prevent infinite re-renders
 * This function caches the ENTIRE result (including arrays) to ensure reference stability
 */
function getTelemetryDataSource(dataSource: DataSourceOrList | undefined): DataSourceOrList | undefined {
  if (!dataSource) return undefined

  // Create a stable cache key from the entire input
  const cacheKey = JSON.stringify(dataSource)

  // Return cached result if exists (reference stability!)
  if (cacheKey in telemetryCache) {
    return telemetryCache[cacheKey]
  }

  const normalizeAndConvert = (ds: DataSource): DataSource => {
    // If already telemetry, return as-is
    if (ds.type === 'telemetry') return ds

    // Convert device type to telemetry for Sparkline
    if (ds.type === 'device' && ds.deviceId && ds.property) {
      return {
        type: 'telemetry',
        deviceId: ds.deviceId,
        metricId: ds.property,
        timeRange: ds.timeRange ?? 1,
        limit: ds.limit ?? 50,
        aggregate: ds.aggregate ?? 'raw',
        refresh: ds.refresh ?? 10,
      }
    }

    return ds
  }

  const result: DataSourceOrList = Array.isArray(dataSource)
    ? dataSource.map(normalizeAndConvert)
    : normalizeAndConvert(dataSource)

  // Cache the entire result for reference stability
  telemetryCache[cacheKey] = result

  return result
}

// Helper function to determine if title should be in display section
// All components show title in the style config, not in the right panel
function isTitleInDisplayComponent(componentType?: string): boolean {
  if (!componentType) return false
  // Components that have their own title input in their config sections
  const titleInConfigTypes: string[] = [
    // Charts - title is in style section
    'line-chart',
    'area-chart',
    'bar-chart',
    'pie-chart',
    // Indicators - title is in style section
    'value-card',
    'counter',
    'metric-card',
    'sparkline',
    'progress-bar',
    'led-indicator',
    // Controls - title is in style section
    'toggle-switch',
    // Display components - title is in style section
    'text-display',
    'image-display',
    'video-display',
    'image-history',
    'web-display',
    'markdown-display',
    'map-display',
  ]
  return titleInConfigTypes.includes(componentType)
}

// ============================================================================
// Component Library Data
// ============================================================================

type ComponentIconType = React.ComponentType<{ className?: string }>

interface ComponentItem {
  id: string
  name: string
  description: string
  icon: ComponentIconType
}

interface ComponentCategory {
  category: string
  categoryLabel: string
  categoryIcon: ComponentIconType
  items: ComponentItem[]
}

const COMPONENT_LIBRARY: ComponentCategory[] = [
  // Indicators & Metrics
  {
    category: 'indicators',
    categoryLabel: 'Indicators',
    categoryIcon: Hash,
    items: [
      { id: 'value-card', name: 'Value Card', description: 'Display a single value', icon: Hash },
      { id: 'led-indicator', name: 'LED Indicator', description: 'LED status light', icon: Circle },
      { id: 'sparkline', name: 'Sparkline', description: 'Mini trend chart', icon: TrendingUp },
      { id: 'progress-bar', name: 'Progress Bar', description: 'Linear progress bar', icon: Layers },
    ],
  },
  // Charts
  {
    category: 'charts',
    categoryLabel: 'Charts',
    categoryIcon: LineChartIcon,
    items: [
      { id: 'line-chart', name: 'Line Chart', description: 'Time series data', icon: LineChartIcon },
      { id: 'area-chart', name: 'Area Chart', description: 'Area under line', icon: LineChartIcon },
      { id: 'bar-chart', name: 'Bar Chart', description: 'Categorical data', icon: BarChart3 },
      { id: 'pie-chart', name: 'Pie Chart', description: 'Part to whole', icon: PieChartIcon },
    ],
  },
  // Display & Content
  {
    category: 'display',
    categoryLabel: 'Display & Content',
    categoryIcon: ImageIcon,
    items: [
      { id: 'image-display', name: 'Image Display', description: 'Display images', icon: ImageIcon },
      { id: 'image-history', name: 'Image History', description: 'Image timeline player', icon: Play },
      { id: 'web-display', name: 'Web Display', description: 'Embed web content', icon: Globe },
      { id: 'markdown-display', name: 'Markdown Display', description: 'Render markdown', icon: FileText },
    ],
  },
  // Spatial & Media
  {
    category: 'spatial',
    categoryLabel: 'Spatial & Media',
    categoryIcon: MapPin,
    items: [
      { id: 'map-display', name: 'Map Display', description: 'Interactive map with markers', icon: MapIcon },
      { id: 'video-display', name: 'Video Display', description: 'Video player and streams', icon: Camera },
      { id: 'custom-layer', name: 'Custom Layer', description: 'Free-form container', icon: SquareIcon },
    ],
  },
  // Controls
  {
    category: 'controls',
    categoryLabel: 'Controls',
    categoryIcon: SlidersHorizontal,
    items: [
      { id: 'toggle-switch', name: 'Toggle Switch', description: 'On/off control', icon: ToggleLeft },
    ],
  },
  // Business Components
  {
    category: 'business',
    categoryLabel: 'Business',
    categoryIcon: Bot,
    items: [
      { id: 'agent-status-card', name: 'Agent Status', description: 'Agent status card', icon: Bot },
      { id: 'decision-list', name: 'Decision List', description: 'Decisions overview', icon: Brain },
      { id: 'device-control', name: 'Device Control', description: 'Device controls', icon: SlidersHorizontal },
      { id: 'rule-status-grid', name: 'Rule Status Grid', description: 'Rules overview', icon: GitBranch },
      { id: 'transform-list', name: 'Transform List', description: 'Data transforms', icon: Workflow },
    ],
  },
]

// ============================================================================
// Render Component
// ============================================================================

// Helper to extract common display props from component config
function getCommonDisplayProps(component: DashboardComponent) {
  const config = (component as any).config || {}

  // Auto-calculate size based on component dimensions
  const w = component.position.w
  const h = component.position.h
  const area = w * h

  // Determine size based on component area
  let calculatedSize: 'xs' | 'sm' | 'md' | 'lg' = 'md'
  if (area <= 1) {
    calculatedSize = 'xs'  // 1x1 grid
  } else if (area <= 2) {
    calculatedSize = 'sm'  // 1x2 or 2x1 grid
  } else if (area <= 4) {
    calculatedSize = 'md'  // 2x2 grid
  } else {
    calculatedSize = 'lg'  // larger than 2x2
  }

  // Use config size if explicitly set, otherwise use calculated size
  const size = config.size || calculatedSize

  return {
    size,
    showCard: config.showCard ?? true,
    className: config.className,
    title: component.title,
    color: config.color,
    // Pass dimensions for responsive adjustments
    dimensions: { w, h, area },
  }
}

// Props that can be safely spread to most components
const getSpreadableProps = (componentType: string, commonProps: ReturnType<typeof getCommonDisplayProps>) => {
  // Components that don't support standard size ('sm' | 'md' | 'lg')
  const noStandardSize = [
    'led-indicator', 'toggle-switch',
    'heading', 'alert-banner',
    'agent-status-card', 'device-control', 'rule-status-grid', 'transform-list',
  ]

  // Components that don't support showCard
  const noShowCard = [
    'value-card', 'led-indicator', 'sparkline', 'progress-bar',
    'toggle-switch',
    'heading', 'alert-banner',
    'agent-status-card', 'device-control', 'rule-status-grid', 'transform-list',
    'tabs',
  ]

  // Components that don't support title in the spread position
  const noTitle = [
    'sparkline', 'led-indicator', 'progress-bar',
    'toggle-switch',
    'heading', 'alert-banner',
    'tabs',
    'agent-status-card', 'device-control', 'rule-status-grid', 'transform-list',
  ]

  const result: Record<string, unknown> = {}

  // Include size for components that support it
  if (!noStandardSize.includes(componentType)) {
    result.size = commonProps.size
  }
  if (!noShowCard.includes(componentType)) {
    result.showCard = commonProps.showCard
  }
  if (!noTitle.includes(componentType)) {
    result.title = commonProps.title
  }
  result.className = commonProps.className
  if (commonProps.color) {
    result.color = commonProps.color
  }

  return result
}

// Calculate chart height based on grid dimensions (each grid row ~120px)
function getChartHeight(component: DashboardComponent): number | 'auto' {
  const h = component.position.h
  // Calculate height: grid rows * 120px - padding (approx 60px for card padding)
  const calculatedHeight = Math.max(h * 120 - 60, 120)
  return calculatedHeight
}

function renderDashboardComponent(component: DashboardComponent, devices: Device[], editMode?: boolean) {
  const config = (component as any).config || {}
  // dataSource is a separate property on GenericComponent, not part of config
  const dataSource = (component as any).dataSource

  const commonProps = getCommonDisplayProps(component)
  const spreadableProps = getSpreadableProps(component.type, commonProps)

  try {
    switch (component.type) {
    // Indicators
    case 'value-card':
      return (
        <ValueCard
          {...spreadableProps}
          dataSource={dataSource}
          title={commonProps.title || 'Value'}
          unit={config.unit}
          prefix={config.prefix}
          icon={config.icon}
          iconType={config.iconType || 'entity'}
          description={config.description}
          variant={config.variant || 'default'}
          iconColor={config.iconColor}
          valueColor={config.valueColor}
          showTrend={config.showTrend}
          trendValue={config.trendValue}
          trendPeriod={config.trendPeriod}
          showSparkline={config.showSparkline}
          sparklineData={config.sparkline}
        />
      )

    case 'led-indicator':
      return (
        <LEDIndicator
          {...spreadableProps}
          dataSource={dataSource}
          state={config.state || 'off'}
          title={config.label || commonProps.title}
          size={config.size || 'md'}
          color={config.color}
          valueMap={config.valueMap}
          defaultState={config.defaultState}
          showGlow={config.showGlow ?? true}
          showAnimation={config.showAnimation ?? true}
          showCard={config.showCard ?? true}
        />
      )

    case 'sparkline':
      return (
        <Sparkline
          {...spreadableProps}
          dataSource={getTelemetryDataSource(dataSource)}
          data={config.data}
          showCard={commonProps.showCard}
          showThreshold={config.showThreshold ?? false}
          threshold={config.threshold}
          thresholdColor={config.thresholdColor}
          title={commonProps.title}
          color={config.color}
          colorMode={config.colorMode || 'auto'}
          fill={config.fill ?? true}
          fillColor={config.fillColor}
          showPoints={config.showPoints ?? false}
          strokeWidth={config.strokeWidth}
          curved={config.curved ?? true}
          showValue={config.showValue}
          maxValue={config.maxValue}
        />
      )

    case 'progress-bar':
      return (
        <ProgressBar
          {...spreadableProps}
          dataSource={dataSource}
          value={dataSource ? undefined : config.value}
          max={config.max ?? 100}
          title={config.label || commonProps.title}
          color={config.color}
          size={config.size || commonProps.size}
          variant={config.variant || 'default'}
          warningThreshold={config.warningThreshold}
          dangerThreshold={config.dangerThreshold}
          dataMapping={config.dataMapping}
          showCard={config.showCard ?? true}
        />
      )

    // Charts
    case 'line-chart':
      return (
        <LineChart
          {...spreadableProps}
          dataSource={dataSource}
          dataMapping={config.dataMapping}
          series={config.series || [{
            name: 'Value',
            data: [20, 22, 21, 24, 23, 26, 25, 28, 27, 30],
            color: '#3b82f6'
          }]}
          labels={config.labels || ['1h', '2h', '3h', '4h', '5h', '6h', '7h', '8h', '9h', '10h']}
          height={getChartHeight(component)}
          title={commonProps.title}
          limit={config.limit}
          timeRange={config.timeRange}
          showGrid={config.showGrid ?? true}
          showLegend={config.showLegend ?? false}
          showTooltip={config.showTooltip ?? true}
          smooth={config.smooth ?? true}
          fillArea={config.fillArea ?? false}
          color={config.color}
          size={config.size}
        />
      )

    case 'area-chart':
      return (
        <AreaChart
          {...spreadableProps}
          dataSource={dataSource}
          dataMapping={config.dataMapping}
          series={config.series || [{
            name: 'Value',
            data: [20, 22, 21, 24, 23, 26, 25, 28, 27, 30],
            color: '#3b82f6'
          }]}
          labels={config.labels || ['1h', '2h', '3h', '4h', '5h', '6h', '7h', '8h', '9h', '10h']}
          height={getChartHeight(component)}
          title={commonProps.title}
          limit={config.limit}
          timeRange={config.timeRange}
          showGrid={config.showGrid ?? true}
          showLegend={config.showLegend ?? false}
          showTooltip={config.showTooltip ?? true}
          smooth={config.smooth ?? true}
          color={config.color}
          size={config.size}
        />
      )

    case 'bar-chart':
      return (
        <BarChart
          {...spreadableProps}
          dataSource={dataSource}
          dataMapping={config.dataMapping}
          data={config.data}
          title={commonProps.title}
          height={getChartHeight(component)}
          limit={config.limit}
          timeRange={config.timeRange}
          showGrid={config.showGrid ?? true}
          showLegend={config.showLegend ?? false}
          showTooltip={config.showTooltip ?? true}
          layout={config.layout || 'vertical'}
          stacked={config.stacked ?? false}
        />
      )

    case 'pie-chart':
      return (
        <PieChart
          {...spreadableProps}
          dataSource={dataSource}
          dataMapping={config.dataMapping}
          data={config.data}
          title={commonProps.title}
          height={getChartHeight(component)}
          limit={config.limit}
          timeRange={config.timeRange}
          showLegend={config.showLegend ?? false}
          showTooltip={config.showTooltip ?? true}
          showLabels={config.showLabels ?? false}
          variant={config.variant || 'donut'}
          innerRadius={config.innerRadius}
          outerRadius={config.outerRadius}
        />
      )

    // Controls
    case 'toggle-switch':
      return (
        <ToggleSwitch
          {...spreadableProps}
          size={config.size || commonProps.size === 'xs' ? 'sm' : commonProps.size}
          dataSource={dataSource}
          title={config.label || commonProps.title}
          initialState={config.initialState ?? false}
          editMode={editMode}
        />
      )

    // Display & Content
    case 'image-display':
      return (
        <ImageDisplay
          {...spreadableProps}
          dataSource={dataSource}
          src={config.src}
          alt={config.alt || commonProps.title || 'Image'}
          caption={config.caption}
          fit={config.fit || 'contain'}
          rounded={config.rounded ?? true}
          showShadow={config.showShadow}
          zoomable={config.zoomable ?? true}
          downloadable={config.downloadable}
        />
      )

    case 'image-history':
      return (
        <ImageHistory
          {...spreadableProps}
          dataSource={dataSource}
          images={config.images}
          fit={config.fit || 'fill'}
          rounded={config.rounded ?? true}
          limit={config.limit ?? 50}
          timeRange={config.timeRange ?? 1}
        />
      )

    case 'web-display':
      return (
        <WebDisplay
          {...spreadableProps}
          dataSource={dataSource}
          src={config.src}
          title={config.title || commonProps.title}
          sandbox={config.sandbox ?? true}
          allowFullscreen={config.allowFullscreen ?? true}
          showHeader={config.showHeader ?? true}
          showUrlBar={config.showUrlBar}
        />
      )

    case 'markdown-display':
      return (
        <MarkdownDisplay
          {...spreadableProps}
          dataSource={dataSource}
          content={config.content}
          variant={config.variant || 'default'}
        />
      )

    case 'video-display':
      return (
        <VideoDisplay
          {...spreadableProps}
          dataSource={dataSource}
          src={config.src}
          type={config.type}
          size={config.size}
          autoplay={config.autoplay ?? false}
          muted={config.muted ?? true}
          controls={config.controls ?? true}
          loop={config.loop ?? false}
          fit={config.fit || 'contain'}
          rounded={config.rounded ?? true}
          showFullscreen={config.showFullscreen ?? true}
        />
      )

    case 'map-display':
      // Convert bindings to markers format for MapDisplay
      console.log('=== Rendering map-display ===')
      console.log('component.id:', component.id)
      console.log('config keys:', Object.keys(config))
      console.log('config.bindings:', config.bindings)
      console.log('config.bindings length:', config.bindings?.length)
      console.log('dataSource:', dataSource)
      console.log('==========================')

      // Get devices from store for metric values and names
      // Use devices from store hook instead of getState() to ensure reactivity
      const storeDevices = devices

      // Log all device IDs in store for debugging
      console.log('[Map] Available devices in store:', storeDevices.map(d => ({ id: d.id, device_id: d.device_id, name: d.name, online: d.online })))

      // Helper to get device name
      const getDeviceName = (deviceId: string) => {
        const device = storeDevices.find(d => d.id === deviceId || d.device_id === deviceId)
        return device?.name || device?.device_id || deviceId
      }

      // Helper to get device status
      const getDeviceStatus = (deviceId: string): 'online' | 'offline' | 'error' | 'warning' | undefined => {
        const device = storeDevices.find(d => d.id === deviceId || d.device_id === deviceId)
        if (!device) return undefined
        return device.online ? 'online' : 'offline'
      }

      const bindingsMarkers = (config.bindings as MapBinding[])?.map((binding): MapMarker => {
        // Get type from icon first, then fallback to type
        const markerType = binding.icon || binding.type
        const ds = binding.dataSource as any

        // Get the device for this binding (used for status, metric values, names)
        const device = ds?.deviceId ? storeDevices.find(d => d.id === ds.deviceId || d.device_id === ds.deviceId) : undefined

        // Get metric value for metric bindings
        let metricValue: string | undefined = undefined
        if (binding.type === 'metric' && ds?.deviceId) {
          const metricKey = ds.metricId || ds.property
          if (device?.current_values && metricKey) {
            const rawValue = device.current_values[metricKey]
            if (rawValue !== undefined && rawValue !== null) {
              metricValue = typeof rawValue === 'number'
                ? rawValue.toFixed(1)
                : String(rawValue)
            } else {
              // Value not found - log for debugging
              console.log('[Map] Metric value not found:', {
                deviceId: ds.deviceId,
                metricKey,
                availableKeys: Object.keys(device.current_values),
                current_values: device.current_values,
              })
            }
          } else {
            console.log('[Map] Device or current_values not found:', {
            deviceId: ds.deviceId,
            deviceFound: !!device,
            hasCurrentValues: !!device?.current_values,
            metricKey,
          })
          }
        }

        return {
          id: binding.id,
          latitude: binding.position === 'auto' || !binding.position
            ? (config.center as { lat: number; lng: number })?.lat ?? 39.9042
            : binding.position.lat,
          longitude: binding.position === 'auto' || !binding.position
            ? (config.center as { lat: number; lng: number })?.lng ?? 116.4074
            : binding.position.lng,
          label: binding.name,
          markerType,
          // Device-specific fields - use actual device status
          deviceId: ds?.deviceId,
          status: binding.type === 'device' ? getDeviceStatus(ds.deviceId) : undefined,
          // Metric-specific fields
          metricValue: binding.type === 'metric' ? (metricValue || '--') : undefined,
          // Command-specific fields
          command: binding.type === 'command' ? ds?.command : undefined,
          // Names for display
          deviceName: ds?.deviceId ? getDeviceName(ds.deviceId) : undefined,
          metricName: ds?.metricId || ds?.property,
          commandName: binding.type === 'command' ? ds?.command : undefined,
        }
      }) ?? []

      console.log('Generated bindingsMarkers:', bindingsMarkers)
      console.log('config.markers:', config.markers)

      // Use bindings markers if available, otherwise fallback to config.markers or empty array
      const displayMarkers = bindingsMarkers.length > 0 ? bindingsMarkers : (config.markers as MapMarker[] || [])
      console.log('Final displayMarkers:', displayMarkers)

      return (
        <MapDisplay
          {...spreadableProps}
          dataSource={dataSource}
          markers={displayMarkers}
          center={config.center}
          zoom={config.zoom}
          minZoom={config.minZoom}
          maxZoom={config.maxZoom}
          showControls={config.showControls ?? true}
          showFullscreen={config.showFullscreen ?? true}
          interactive={config.interactive ?? true}
          tileLayer={config.tileLayer || 'osm'}
          deviceBinding={config.deviceBinding}
        />
      )

    case 'custom-layer':
      return (
        <CustomLayer
          {...spreadableProps}
          bindings={config.bindings}
          backgroundType={config.backgroundType || 'grid'}
          backgroundColor={config.backgroundColor}
          backgroundImage={config.backgroundImage}
          gridSize={config.gridSize}
          showControls={config.showControls ?? true}
          showFullscreen={config.showFullscreen ?? true}
          interactive={config.interactive ?? true}
          editable={config.editable}
        />
      )

    // Business Components (not implemented)
    case 'agent-status-card':
    case 'decision-list':
    case 'device-control':
    case 'rule-status-grid':
    case 'transform-list':
      return (
        <div className="p-4 text-center text-muted-foreground h-full flex flex-col items-center justify-center">
          <p className="text-sm font-medium">{component.type}</p>
          <p className="text-xs mt-1">This component is not yet implemented</p>
        </div>
      )

    default:
      return (
        <div className="p-4 text-center text-muted-foreground h-full flex flex-col items-center justify-center">
          <p className="text-sm font-medium">{(component as any).type}</p>
          <p className="text-xs mt-1">Component not implemented</p>
        </div>
      )
  }
  } catch (error) {
    console.error(`Error rendering component ${(component as any).type}:`, error)
    return (
      <div className="p-4 text-center text-destructive h-full flex flex-col items-center justify-center bg-destructive/10 rounded-lg">
        <p className="text-sm font-medium">{(component as any).type}</p>
        <p className="text-xs mt-1">Error loading component</p>
      </div>
    )
  }
}

// ============================================================================
// Component Wrapper with Edit Mode Actions
// ============================================================================

interface ComponentWrapperProps {
  component: DashboardComponent
  children: React.ReactNode
  editMode: boolean
  onOpenConfig: (componentId: string) => void
  onRemove: (componentId: string) => void
  onDuplicate: (componentId: string) => void
}

function ComponentWrapper({
  component,
  children,
  editMode,
  onOpenConfig,
  onRemove,
  onDuplicate,
}: ComponentWrapperProps) {
  const [isHovered, setIsHovered] = useState(false)

  return (
    <div
      className="relative h-full"
      onMouseEnter={() => setIsHovered(true)}
      onMouseLeave={() => setIsHovered(false)}
    >
      {/* Component content */}
      <div className="h-full w-full flex flex-col">
        {children}
      </div>

      {/* Edit mode overlay */}
      {editMode && (isHovered || window.matchMedia('(hover: none)').matches) && (
        <div className="absolute top-2 right-2 z-10 flex gap-1">
          <Button
            variant="secondary"
            size="icon"
            className="h-7 w-7 bg-background/90 backdrop-blur"
            onClick={() => onOpenConfig(component.id)}
          >
            <Settings2 className="h-3.5 w-3.5" />
          </Button>
          <Button
            variant="secondary"
            size="icon"
            className="h-7 w-7 bg-background/90 backdrop-blur"
            onClick={() => onDuplicate(component.id)}
          >
            <Copy className="h-3.5 w-3.5" />
          </Button>
          <Button
            variant="secondary"
            size="icon"
            className="h-7 w-7 bg-background/90 backdrop-blur hover:bg-destructive hover:text-destructive-foreground transition-colors"
            onClick={() => onRemove(component.id)}
          >
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
        </div>
      )}
    </div>
  )
}

// ============================================================================
// Main Component
// ============================================================================

export function VisualDashboard() {
  const { dashboardId } = useParams<{ dashboardId?: string }>()
  const navigate = useNavigate()
  const { t } = useTranslation('dashboardComponents')

  const {
    currentDashboard,
    currentDashboardId,
    dashboards,
    dashboardsLoading,
    devices,
    editMode,
    setEditMode,
    addComponent,
    updateComponent,
    removeComponent,
    duplicateComponent,
    createDashboard,
    updateDashboard,
    deleteDashboard,
    persistDashboard,
    setCurrentDashboard,
    componentLibraryOpen,
    setComponentLibraryOpen,
    fetchDashboards,
    fetchDevices,
    fetchDeviceTypes,
    fetchDevicesCurrentBatch,
  } = useStore()

  const [configOpen, setConfigOpen] = useState(false)
  const [selectedComponent, setSelectedComponent] = useState<DashboardComponent | null>(null)

  // Map editor dialog state
  const [mapEditorOpen, setMapEditorOpen] = useState(false)
  const [mapEditorBindings, setMapEditorBindings] = useState<MapBinding[]>([])

  // Layer editor dialog state
  const [layerEditorOpen, setLayerEditorOpen] = useState(false)
  const [layerEditorBindings, setLayerEditorBindings] = useState<LayerBinding[]>([])

  // Persist sidebar state to localStorage
  const [sidebarOpen, setSidebarOpen] = useState(() => {
    const saved = localStorage.getItem('neotalk_dashboard_sidebar_open')
    return saved !== 'false' // Default to true
  })

  // Update localStorage when sidebar state changes
  const handleSidebarOpenChange = useCallback((open: boolean) => {
    setSidebarOpen(open)
    localStorage.setItem('neotalk_dashboard_sidebar_open', String(open))
  }, [])

  // Dashboard list handlers
  const handleDashboardSwitch = useCallback((id: string) => {
    setCurrentDashboard(id)
  }, [setCurrentDashboard])

  const handleDashboardCreate = useCallback((name: string) => {
    createDashboard({
      name,
      layout: {
        columns: 12,
        rows: 'auto' as const,
        breakpoints: { lg: 1200, md: 996, sm: 768, xs: 480 },
      },
      components: [],
    })
  }, [createDashboard])

  const handleDashboardRename = useCallback((id: string, name: string) => {
    updateDashboard(id, { name })
  }, [updateDashboard])

  const handleDashboardDelete = useCallback(async (id: string) => {
    const confirmed = await confirm({
      title: 'Delete Dashboard',
      description: 'Delete this dashboard?',
      confirmText: 'Delete',
      cancelText: 'Cancel',
      variant: 'destructive'
    })
    if (confirmed) {
      deleteDashboard(id)
    }
  }, [deleteDashboard])

  // Config dialog state
  const [configTitle, setConfigTitle] = useState('')
  const [componentConfig, setComponentConfig] = useState<Record<string, any>>({})
  const [configSchema, setConfigSchema] = useState<ComponentConfigSchema | null>(null)

  // Store original config for revert on cancel
  const [originalComponentConfig, setOriginalComponentConfig] = useState<Record<string, any>>({})
  const [originalTitle, setOriginalTitle] = useState('')

  // Track if we've initialized to avoid duplicate calls
  const hasInitialized = useRef(false)

  // Track previous components to detect actual changes (not just reference changes)
  const prevComponentsRef = useRef<DashboardComponent[]>([])

  // Use a counter to force refresh when config changes in dialog
  // Must be declared before componentsStableKey which depends on it
  const [configVersion, setConfigVersion] = useState(0)

  // Create a stable key for components to detect actual changes
  // This key only changes when component data actually changes, not on every render
  const componentsStableKey = useMemo(() => {
    const components = currentDashboard?.components ?? []
    const prevComponents = prevComponentsRef.current ?? []

    // Quick check: if length changed, definitely different
    if (components.length !== prevComponents.length) {
      prevComponentsRef.current = components
      return `changed-${components.length}-${Date.now()}-${configVersion}`
    }

    // Deep check: compare each component's key properties
    for (let i = 0; i < components.length; i++) {
      const curr = components[i]
      const prev = prevComponents[i]

      if (!prev) {
        prevComponentsRef.current = components
        return `new-${curr.id}-${curr.type}-${Date.now()}-${configVersion}`
      }

      // Check each property separately (including title and dataSource)
      if (curr.id !== prev.id ||
          curr.type !== prev.type ||
          curr.title !== prev.title ||
          curr.position.x !== prev.position.x ||
          curr.position.y !== prev.position.y ||
          curr.position.w !== prev.position.w ||
          curr.position.h !== prev.position.h ||
          JSON.stringify(curr.config) !== JSON.stringify(prev.config) ||
          JSON.stringify((curr as any).dataSource) !== JSON.stringify((prev as any).dataSource)) {
        prevComponentsRef.current = components
        return `changed-${curr.id}-${Date.now()}-${configVersion}`
      }
    }

    // No actual changes detected - return stable key with version
    return `stable-${components.length}-${configVersion}`
  }, [currentDashboard?.components, configVersion])

  // Initialize dashboards on mount
  useEffect(() => {
    if (hasInitialized.current) return
    hasInitialized.current = true

    // Fetch dashboards (handles both localStorage and API)
    fetchDashboards()
    // Fetch devices and device types so they're available for data binding
    fetchDevices()
    fetchDeviceTypes()
  }, [fetchDashboards, fetchDevices, fetchDeviceTypes])

  // Batch fetch current values for devices used in dashboard components
  // This ensures dashboard components have current_values after server restart
  useEffect(() => {
    if (devices.length === 0 || !currentDashboard) {
      return
    }

    // Extract all unique device IDs from dashboard components
    const deviceIds = new Set<string>()
    for (const dashboard of dashboards) {
      for (const component of dashboard.components) {
        const genericComponent = component as GenericComponent
        const dataSource = genericComponent.dataSource
        if (dataSource?.deviceId) {
          deviceIds.add(dataSource.deviceId)
        }
        // Also check for devices in map-display bindings
        if (genericComponent.type === 'map-display') {
          const bindings = (genericComponent.config as any)?.bindings as MapBinding[] || []
          for (const binding of bindings) {
            const ds = binding.dataSource as any
            if (ds?.deviceId) {
              deviceIds.add(ds.deviceId)
            }
          }
        }
      }
    }

    if (deviceIds.size > 0) {
      console.log('[VisualDashboard] Fetching current values for', deviceIds.size, 'devices')
      fetchDevicesCurrentBatch(Array.from(deviceIds))
    }
  }, [devices.length, dashboards, currentDashboard, fetchDevicesCurrentBatch])

  // Initialize: fetch dashboards on mount
  useEffect(() => {
    fetchDashboards()
  }, [])

  // Re-load dashboards if array becomes empty but we have a current ID
  useEffect(() => {
    if (dashboards.length === 0 && currentDashboardId) {
      // Try to recover by fetching again
      fetchDashboards()
    }
  }, [dashboards.length, currentDashboardId, fetchDashboards])

  // Note: Removed auto-create dashboard logic
  // Users should explicitly create dashboards via the UI
  // This prevents creating duplicate dashboards on refresh

  // Handle adding a component
  const handleAddComponent = (componentType: string) => {
    const item = COMPONENT_LIBRARY
      .flatMap(cat => cat.items)
      .find(i => i.id === componentType)

    // Get size constraints for this component type
    const constraints = COMPONENT_SIZE_CONSTRAINTS[componentType as keyof typeof COMPONENT_SIZE_CONSTRAINTS]

    // Build appropriate default config based on component type
    let defaultConfig: any = {}

    switch (componentType) {
      // Charts
      case 'line-chart':
      case 'area-chart':
        defaultConfig = {
          series: [{ name: 'Value', data: [10, 25, 15, 30, 28, 35, 20], color: '#3b82f6' }],
          labels: ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']
        }
        break
      case 'bar-chart':
        defaultConfig = {
          data: [{ name: 'A', value: 30 }, { name: 'B', value: 50 }, { name: 'C', value: 20 }]
        }
        break
      case 'pie-chart':
        defaultConfig = {
          data: [{ name: 'A', value: 30 }, { name: 'B', value: 50 }, { name: 'C', value: 20 }]
        }
        break
      // Indicators
      case 'sparkline':
        defaultConfig = {
          data: [12, 19, 15, 25, 22, 30, 28]
        }
        break
      case 'progress-bar':
        defaultConfig = {
          value: 65,
          min: 0,
          max: 100
        }
        break
      case 'led-indicator':
        defaultConfig = {
          state: 'on'
        }
        break
      // Controls
      case 'toggle-switch':
        defaultConfig = {
          initialState: false
        }
        break
      // Display & Content
      case 'image-display':
        defaultConfig = {
          src: 'https://via.placeholder.com/400x200',
          alt: 'Sample Image',
          fit: 'contain',
          rounded: true,
          zoomable: true,
        }
        break
      case 'image-history':
        defaultConfig = {
          dataSource: {
            type: 'static',
            staticValue: [
              { src: 'https://via.placeholder.com/400x200/8b5cf6/ffffff?text=Image+1', timestamp: Date.now() - 6000 },
              { src: 'https://via.placeholder.com/400x200/22c55e/ffffff?text=Image+2', timestamp: Date.now() - 4000 },
              { src: 'https://via.placeholder.com/400x200/f59e0b/ffffff?text=Image+3', timestamp: Date.now() - 2000 },
              { src: 'https://via.placeholder.com/400x200/ec4899/ffffff?text=Image+4', timestamp: Date.now() },
            ],
          },
          fit: 'fill',
          rounded: true,
          limit: 50,
          timeRange: 1,
        }
        break
      case 'web-display':
        defaultConfig = {
          src: 'https://example.com',
          title: 'Website',
          sandbox: true,
          showHeader: true,
        }
        break
      case 'markdown-display':
        defaultConfig = {
          content: '# Title\n\nThis is **markdown** content.\n\n- Item 1\n- Item 2\n\n`code example`',
          variant: 'default',
        }
        break
      case 'video-display':
        defaultConfig = {
          src: '',
          type: 'file',
          autoplay: false,
          muted: true,
          controls: true,
          loop: false,
          fit: 'contain',
          rounded: true,
          showFullscreen: true,
        }
        break
      case 'map-display':
        defaultConfig = {
          center: { lat: 39.9042, lng: 116.4074 }, // Beijing
          zoom: 10,
          minZoom: 2,
          maxZoom: 18,
          showControls: true,
          showLayers: true,
          showFullscreen: true,
          interactive: true,
          tileLayer: 'osm',
          markers: [
            { id: '1', latitude: 39.9042, longitude: 116.4074, label: 'Beijing', status: 'online' },
            { id: '2', latitude: 31.2304, longitude: 121.4737, label: 'Shanghai', status: 'online' },
            { id: '3', latitude: 23.1291, longitude: 113.2644, label: 'Guangzhou', status: 'warning' },
          ],
        }
        break
      case 'custom-layer':
        defaultConfig = {
          backgroundType: 'grid',
          gridSize: 20,
          showControls: true,
          showFullscreen: true,
          interactive: true,
          editable: false,
        }
        break
      // Business Components (not implemented)
      case 'agent-status-card':
      case 'decision-list':
      case 'device-control':
      case 'rule-status-grid':
      case 'transform-list':
        defaultConfig = {}
        break
      default:
        defaultConfig = {}
    }

    // Calculate position for new component to avoid overlap
    // Find the next available position using a simple grid packing algorithm
    const components = currentDashboard?.components ?? []
    const w = constraints?.defaultW ?? 4
    const h = constraints?.defaultH ?? 3

    // Simple grid packing: place components row by row
    let x = 0
    let y = 0
    const maxCols = 12  // Base grid columns

    // Build a simple map of occupied positions
    const occupied = new Set<string>()
    components.forEach(c => {
      for (let dy = 0; dy < c.position.h; dy++) {
        for (let dx = 0; dx < c.position.w; dx++) {
          occupied.add(`${c.position.x + dx},${c.position.y + dy}`)
        }
      }
    })

    // Find first available position
    let found = false
    while (!found) {
      // Check if current position is free
      let canFit = true
      for (let dy = 0; dy < h && canFit; dy++) {
        for (let dx = 0; dx < w && canFit; dx++) {
          if (occupied.has(`${x + dx},${y + dy}`)) {
            canFit = false
          }
        }
      }

      if (canFit) {
        found = true
      } else {
        // Move to next position
        x += w
        if (x + w > maxCols) {
          x = 0
          y += 1
        }
      }
    }

    const newComponent: Omit<DashboardComponent, 'id'> = {
      type: componentType as any,
      position: {
        x,
        y,
        w,
        h,
        minW: constraints?.minW,
        minH: constraints?.minH,
        maxW: constraints?.maxW,
        maxH: constraints?.maxH,
      },
      title: item?.name || componentType,
      config: defaultConfig,
    }

    addComponent(newComponent)
    setComponentLibraryOpen(false)
  }

  // Handle layout change
  const handleLayoutChange = (layout: readonly any[]) => {
    console.log('[VisualDashboard] handleLayoutChange called with:', layout)
    layout.forEach((item) => {
      console.log(`[VisualDashboard] Updating component ${item.i}:`, { x: item.x, y: item.y, w: item.w, h: item.h })
      updateComponent(item.i, {
        position: {
          x: item.x,
          y: item.y,
          w: item.w,
          h: item.h,
        },
      })
    })
  }

  // Handle opening config dialog
  const handleOpenConfig = useCallback((componentId: string) => {
    const component = currentDashboard?.components.find(c => c.id === componentId)
    if (!component) return

    setSelectedComponent(component)
    // Extract both config and dataSource (they are separate properties on GenericComponent)
    const config = { ...((component as any).config || {}) }
    const dataSource = (component as any).dataSource
    // Include title in config so style sections can access it
    const configWithTitle = { ...config, title: component.title }
    // Merge dataSource into config for unified state management
    const mergedConfig = dataSource ? { ...configWithTitle, dataSource } : configWithTitle

    // Store original config for revert on cancel
    setOriginalComponentConfig(mergedConfig)
    setOriginalTitle(component.title || '')

    setConfigTitle(component.title || '')
    setComponentConfig(mergedConfig)
    setConfigOpen(true)
  }, [currentDashboard?.components])

  // ============================================================================
  // Unified Select Field Helper
  // ============================================================================

  interface SelectOption {
    value: string
    label: string
  }

  interface SelectFieldProps {
    label: string
    value: string
    onChange: (value: string) => void
    options: SelectOption[]
    className?: string
  }

  // SelectField component - NOT memoized to ensure it receives fresh props
  function SelectField({ label, value, onChange, options, className }: SelectFieldProps) {
    console.log('[SelectField render] label:', label, 'value:', value)
    const handleChange = (newValue: string) => {
      console.log('[SelectField handleChange] newValue:', newValue, 'label:', label)
      onChange(newValue)
    }
    return (
      <Field className={className}>
        <Label>{label}</Label>
        <Select value={value} onValueChange={handleChange}>
          <SelectTrigger>
            <SelectValue placeholder={t('visualDashboard.selectPlaceholder', { label })} />
          </SelectTrigger>
          <SelectContent>
            {options.map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </Field>
    )
  }

  // Memoize grid components to prevent infinite re-renders
  // Only recalculate when actual component data changes (detected via stableKey)
  // Note: handleOpenConfig, removeComponent, duplicateComponent are NOT dependencies
  // because they don't affect the rendered output structure, only event handlers
  // devices.length is included to ensure re-render when devices are initially loaded
  // IMPORTANT: Use currentDashboard from props (reactive) to ensure updates are reflected
  const gridComponents = useMemo(() => {
    return currentDashboard?.components.map((component) => {
      // Get dataSource from component (it should be a separate property, not in config)
      const componentDataSource = (component as any).dataSource

      return {
        id: component.id,
        position: component.position,
        children: (
          <ComponentWrapper
            key={component.id}
            component={component}
            editMode={editMode}
            onOpenConfig={handleOpenConfig}
            onRemove={removeComponent}
            onDuplicate={duplicateComponent}
          >
            {renderDashboardComponent(component, devices, editMode)}
          </ComponentWrapper>
        ),
      }
    }) ?? []
  }, [componentsStableKey, editMode, configVersion, devices.length])

  // Track initial config load to avoid unnecessary updates
  const initialConfigRef = useRef<any>(null)
  const isInitialLoad = useRef(false)
  const lastSyncedConfigRef = useRef<string>('')

  // Live preview: update component in real-time as config changes
  useEffect(() => {
    if (configOpen && selectedComponent) {
      // Skip initial load - don't update store with same config
      if (!isInitialLoad.current) {
        initialConfigRef.current = componentConfig
        isInitialLoad.current = true
        lastSyncedConfigRef.current = JSON.stringify(componentConfig)
        setConfigSchema(generateConfigSchema(selectedComponent.type, componentConfig))
        return
      }

      // Check if config actually changed since last sync
      const currentJSON = JSON.stringify(componentConfig)
      if (currentJSON !== lastSyncedConfigRef.current) {
        // Separate dataSource from config for proper update
        const { dataSource, ...configOnly } = componentConfig
        const currentDS = (selectedComponent as any).dataSource
        const updateData: any = { config: configOnly }
        // Include dataSource if:
        // 1. It's defined (has a value), OR
        // 2. It's undefined but the component previously had a dataSource (need to clear it)
        if (dataSource !== undefined || currentDS !== undefined) {
          updateData.dataSource = dataSource
        }
        // Update the component with current config for live preview (don't persist yet)
        updateComponent(selectedComponent.id, updateData, false)
        // Update last synced config
        lastSyncedConfigRef.current = currentJSON
        // Increment version to force re-render
        setConfigVersion(v => v + 1)
        // Regenerate schema with new config values
        setConfigSchema(generateConfigSchema(selectedComponent.type, componentConfig))
      }
    } else {
      // Reset when dialog closes
      isInitialLoad.current = false
      initialConfigRef.current = null
      lastSyncedConfigRef.current = ''
    }
  }, [componentConfig, configOpen, selectedComponent?.id, updateComponent, setConfigSchema])

  // Handle canceling component config - revert to original
  const handleCancelConfig = useCallback(() => {
    if (selectedComponent && originalComponentConfig) {
      // Revert to original config (no need to persist - reverting to saved state)
      const { dataSource, ...configOnly } = originalComponentConfig
      const currentDS = (selectedComponent as any).dataSource
      const updateData: any = { config: configOnly }
      // Include dataSource if:
      // 1. Original config had dataSource, OR
      // 2. Original config didn't have dataSource but current component does (need to clear it)
      if (dataSource !== undefined || currentDS !== undefined) {
        updateData.dataSource = dataSource
      }
      updateComponent(selectedComponent.id, updateData, false)

      // Revert title
      if (originalTitle !== selectedComponent.title) {
        updateComponent(selectedComponent.id, { title: originalTitle }, false)
      }
    }
    setConfigOpen(false)
  }, [selectedComponent, originalComponentConfig, originalTitle, updateComponent])

  // Handle saving component config - persist the dashboard to localStorage
  const handleSaveConfig = async () => {
    if (selectedComponent) {
      console.log('=== handleSaveConfig ===')

      // Get the latest component from the store to merge with local changes
      const latestDashboard = useStore.getState().currentDashboard
      const latestComponent = latestDashboard?.components.find(c => c.id === selectedComponent.id)

      console.log('selectedComponent.id:', selectedComponent.id)
      console.log('local componentConfig:', componentConfig)
      console.log('latestComponent:', latestComponent)

      // Extract dataSource from multiple possible locations:
      // 1. componentConfig.dataSource (newly selected/changed in config dialog)
      // 2. componentConfig.config.dataSource (from handleOpenConfig merge)
      // 3. mergedConfig.dataSource (already in merged config)
      // 4. latestComponent.dataSource (existing on component, separate property)
      // 5. latestComponent.config.dataSource (existing in component's config)
      const configDataSource = componentConfig.dataSource
      const nestedConfigDataSource = (componentConfig.config as any)?.dataSource
      const latestConfigDataSource = (latestComponent as any)?.config?.dataSource
      const latestComponentDataSource = (latestComponent as any)?.dataSource

      // Priority: componentConfig.dataSource > nested config.dataSource > latest component.dataSource > latest config.dataSource
      const finalDataSource = configDataSource ?? nestedConfigDataSource ?? latestComponentDataSource ?? latestConfigDataSource

      console.log('dataSource sources:', {
        configDataSource,
        nestedConfigDataSource,
        latestConfigDataSource,
        latestComponentDataSource,
        finalDataSource,
      })

      // Merge local config changes with the latest component config
      // Local changes take precedence
      const mergedConfig = {
        ...(latestComponent as any)?.config || {},
        ...componentConfig,
      }

      // IMPORTANT: Remove dataSource from mergedConfig to avoid confusion
      // dataSource should be stored as a separate property, not inside config
      delete (mergedConfig as any).dataSource

      console.log('mergedConfig (after dataSource extraction):', mergedConfig)

      // Update the component in the store
      // CRITICAL: dataSource must be saved as a separate property, not inside config
      const updateData: any = {
        config: mergedConfig,
        title: configTitle,
      }
      if (finalDataSource !== undefined) {
        updateData.dataSource = finalDataSource
      }

      console.log('updateData to save:', updateData)

      updateComponent(selectedComponent.id, updateData, false)

      // Force immediate re-render by incrementing configVersion
      setConfigVersion(v => v + 1)

      // Verify after update
      setTimeout(() => {
        const verifyDashboard = useStore.getState().currentDashboard
        const verifyComponent = verifyDashboard?.components.find(c => c.id === selectedComponent.id)
        console.log('=== After saveConfig verification ===')
        console.log('verifyComponent.config:', (verifyComponent as any)?.config)
        console.log('verifyComponent.dataSource:', (verifyComponent as any)?.dataSource)
      }, 50)
    }
    // Persist all changes to localStorage
    await persistDashboard()
    setConfigOpen(false)
  }

  // Handle saving map editor bindings
  const handleMapEditorSave = async (bindings: MapBinding[]) => {
    console.log('=== handleMapEditorSave ===')
    console.log('bindings received:', bindings)
    console.log('bindings count:', bindings?.length)

    // Fix any duplicate IDs in bindings before saving
    const idCount = new Map<string, number>() as Map<string, number>
    const fixedBindings = bindings.map((binding, index) => {
      const ds = binding.dataSource as any
      const currentId = binding.id
      idCount.set(currentId, (idCount.get(currentId) || 0) + 1)

      // If ID is duplicated, regenerate it
      if (idCount.get(currentId)! > 1) {
        let newId: string
        if (binding.type === 'metric' || ds?.type === 'telemetry') {
          newId = `metric-${ds?.deviceId}-${ds?.metricId || ds?.property || index}`
        } else if (binding.type === 'command') {
          newId = `command-${ds?.deviceId}-${ds?.command}`
        } else {
          newId = `device-${ds?.deviceId}-${index}`
        }
        console.log(`Fixing duplicate ID in save: ${currentId} -> ${newId}`)
        return { ...binding, id: newId }
      }
      return binding
    })

    if (selectedComponent) {
      // CRITICAL FIX: Get the latest component config from the store to avoid stale state
      const latestDashboard = useStore.getState().currentDashboard
      const latestComponent = latestDashboard?.components.find(c => c.id === selectedComponent.id)

      console.log('selectedComponent.id:', selectedComponent.id)
      console.log('latestComponent found:', !!latestComponent)

      const latestConfig = (latestComponent as any)?.config || {}
      const latestDataSource = (latestComponent as any)?.dataSource

      console.log('latestConfig keys:', Object.keys(latestConfig))
      console.log('latestConfig.bindings:', latestConfig.bindings)
      console.log('latestDataSource:', latestDataSource)

      // Merge the latest config with the new bindings, preserving dataSource
      const newConfig = { ...latestConfig, bindings: fixedBindings }
      const updateData: any = { config: newConfig }

      // CRITICAL: Preserve dataSource when updating
      if (latestDataSource) {
        updateData.dataSource = latestDataSource
      }

      console.log('updateData to save:', updateData)
      console.log('updateData.config.bindings:', updateData.config.bindings)

      // Update the store with both config and dataSource
      updateComponent(selectedComponent.id, updateData, false)

      // Force immediate re-render by incrementing configVersion
      setConfigVersion(v => v + 1)

      // Update local config state
      setComponentConfig(prev => ({ ...prev, bindings: fixedBindings }))

      // Verify after update
      setTimeout(() => {
        const verifyDashboard = useStore.getState().currentDashboard
        const verifyComponent = verifyDashboard?.components.find(c => c.id === selectedComponent.id)
        console.log('=== After save verification ===')
        console.log('verifyComponent.config:', (verifyComponent as any)?.config)
        console.log('verifyComponent.config.bindings:', ((verifyComponent as any)?.config)?.bindings)
        console.log('verifyComponent.dataSource:', (verifyComponent as any)?.dataSource)
        console.log('=============================')
      }, 50)
    }

    // Persist to localStorage
    await persistDashboard()

    setMapEditorOpen(false)
  }

  // Handle saving layer editor bindings
  const handleLayerEditorSave = async (bindings: LayerBinding[]) => {
    console.log('=== handleLayerEditorSave ===')
    console.log('bindings received:', bindings)
    console.log('bindings count:', bindings?.length)

    if (selectedComponent) {
      const latestDashboard = useStore.getState().currentDashboard
      const latestComponent = latestDashboard?.components.find(c => c.id === selectedComponent.id)

      const latestConfig = (latestComponent as any)?.config || {}
      const latestDataSource = (latestComponent as any)?.dataSource

      // Merge the latest config with the new bindings, preserving dataSource
      const newConfig = { ...latestConfig, bindings }
      const updateData: any = { config: newConfig }

      // Preserve dataSource when updating
      if (latestDataSource) {
        updateData.dataSource = latestDataSource
      }

      // Update the store
      updateComponent(selectedComponent.id, updateData, false)

      // Force re-render
      setConfigVersion(v => v + 1)

      // Update local config state
      setComponentConfig(prev => ({ ...prev, bindings }))
    }

    // Persist to localStorage
    await persistDashboard()

    setLayerEditorOpen(false)
  }

  // Handle title change
  const handleTitleChange = (newTitle: string) => {
    setConfigTitle(newTitle)
    if (selectedComponent) {
      // Don't persist during edit - will be persisted on save
      updateComponent(selectedComponent.id, { title: newTitle }, false)
    }
  }

  // Generate config schema based on component type
  const generateConfigSchema = (componentType: string, currentConfig: any): ComponentConfigSchema | null => {
    const config = currentConfig || {}

    // Helper to create updater functions
    const updateConfig = (key: string) => (value: any) => {
      console.log('[updateConfig] key:', key, 'value:', value)
      if (key === 'title') {
        // Title changes need to sync with configTitle state and the component
        setConfigTitle(value)
        if (selectedComponent) {
          updateComponent(selectedComponent.id, { title: value }, false)
        }
      }
      setComponentConfig(prev => {
        const updated = { ...prev, [key]: value }
        console.log('[updateConfig] componentConfig updated:', updated)
        return updated
      })
    }

    const updateNestedConfig = (parent: string, key: string) => (value: any) => {
      setComponentConfig(prev => ({
        ...prev,
        [parent]: { ...prev[parent], [key]: value }
      }))
    }

    // Data source updater
    const updateDataSource = (ds: any) => {
      setComponentConfig(prev => ({ ...prev, dataSource: ds }))
    }

    // Data mapping updater
    const updateDataMapping = (newMapping: any) => {
      setComponentConfig(prev => ({ ...prev, dataMapping: newMapping }))
    }

    switch (componentType) {
      // ========== Indicators ==========
      case 'value-card':
      case 'counter':
      case 'metric-card':
        return {
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <Field>
                    <Label htmlFor="value-card-title">{t('visualDashboard.showTitle')}</Label>
                    <Input
                      id="value-card-title"
                      value={config.title as string || ''}
                      onChange={(e) => updateConfig('title')(e.target.value)}
                      placeholder={t('visualDashboard.enterTitle')}
                      className="h-10"
                    />
                  </Field>

                  <SelectField
                    label={t('visualDashboard.style')}
                    value={config.variant || 'default'}
                    onChange={updateConfig('variant')}
                    options={[
                      { value: 'default', label: t('visualDashboard.default') },
                      { value: 'vertical', label: t('visualDashboard.vertical') },
                      { value: 'compact', label: t('visualDashboard.compact') },
                      { value: 'minimal', label: t('visualDashboard.minimal') },
                    ]}
                  />

                  <EntityIconPicker
                    value={config.icon || ''}
                    onChange={(icon) => updateConfig('icon')(icon)}
                    label={t('visualDashboard.icon')}
                  />

                  <ColorPicker
                    value={config.iconColor || '#3b82f6'}
                    onChange={(color) => updateConfig('iconColor')(color)}
                    label={t('visualDashboard.iconColor')}
                    presets="primary"
                  />

                  <ColorPicker
                    value={config.valueColor || '#3b82f6'}
                    onChange={(color) => updateConfig('valueColor')(color)}
                    label={t('visualDashboard.valueColor')}
                    presets="primary"
                  />

                  <div className="grid grid-cols-2 gap-3">
                    <Field>
                      <Label>{t('visualDashboard.prefix')}</Label>
                      <Input
                        value={config.prefix || ''}
                        onChange={(e) => updateConfig('prefix')(e.target.value)}
                        placeholder={t('visualDashboard.prefixPlaceholder')}
                        className="h-9"
                      />
                    </Field>

                    <Field>
                      <Label>{t('visualDashboard.unit')}</Label>
                      <Input
                        value={config.unit || ''}
                        onChange={(e) => updateConfig('unit')(e.target.value)}
                        placeholder={t('visualDashboard.unitPlaceholder')}
                        className="h-9"
                      />
                    </Field>
                  </div>

                  <Field>
                    <Label>{t('visualDashboard.description')}</Label>
                    <Input
                      value={config.description || ''}
                      onChange={(e) => updateConfig('description')(e.target.value)}
                      placeholder={t('visualDashboard.descriptionPlaceholder')}
                      className="h-9"
                    />
                  </Field>

                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showTrend ?? false}
                        onChange={(e) => updateConfig('showTrend')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">{t('visualDashboard.showTrend')}</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showSparkline ?? false}
                        onChange={(e) => updateConfig('showSparkline')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">{t('visualDashboard.showSparkline')}</span>
                    </label>
                  </div>

                  {config.showTrend && (
                    <Field>
                      <Label>{t('visualDashboard.trendValue')}</Label>
                      <Input
                        type="number"
                        value={config.trendValue ?? 0}
                        onChange={(e) => updateConfig('trendValue')(Number(e.target.value))}
                        className="h-9"
                      />
                    </Field>
                  )}
                </div>
              ),
            },
          ],
          displaySections: [],
          dataSourceSections: [
            {
              type: 'data-source' as const,
              props: {
                dataSource: config.dataSource,
                onChange: updateDataSource,
                allowedTypes: ['device-metric'],
              },
            },
          ],
        }

      case 'sparkline':
        return {
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <Field>
                    <Label htmlFor="sparkline-title">{t('visualDashboard.showTitle')}</Label>
                    <Input
                      id="sparkline-title"
                      value={config.title as string || ''}
                      onChange={(e) => updateConfig('title')(e.target.value)}
                      placeholder={t('visualDashboard.enterTitle')}
                      className="h-10"
                    />
                  </Field>

                  <SelectField
                    label={t('visualDashboard.colorMode')}
                    value={config.colorMode || 'auto'}
                    onChange={updateConfig('colorMode')}
                    options={[
                      { value: 'auto', label: t('visualDashboard.auto') },
                      { value: 'primary', label: t('visualDashboard.primaryColor') },
                      { value: 'fixed', label: t('visualDashboard.fixedColor') },
                      { value: 'value', label: t('visualDashboard.basedOnValue') },
                    ]}
                  />

                  <ColorPicker
                    value={config.color || '#3b82f6'}
                    onChange={(color) => updateConfig('color')(color)}
                    label={t('visualDashboard.fixedModeColor')}
                    presets="primary"
                  />

                  <Field>
                    <Label>{t('visualDashboard.maxValue')}</Label>
                    <Input
                      type="number"
                      value={config.maxValue || 100}
                      onChange={(e) => updateConfig('maxValue')(Number(e.target.value))}
                      min={1}
                      className="h-9"
                    />
                  </Field>

                  <Field>
                    <Label>{t('visualDashboard.lineWidth')}</Label>
                    <Input
                      type="number"
                      value={config.strokeWidth ?? 2}
                      onChange={(e) => updateConfig('strokeWidth')(Number(e.target.value))}
                      min={1}
                      max={5}
                      className="h-9"
                    />
                  </Field>

                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.fill ?? true}
                        onChange={(e) => updateConfig('fill')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">{t('visualDashboard.fillArea')}</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.curved ?? true}
                        onChange={(e) => updateConfig('curved')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">{t('visualDashboard.curved')}</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showPoints ?? false}
                        onChange={(e) => updateConfig('showPoints')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">{t('visualDashboard.showDataPoints')}</span>
                    </label>
                  </div>

                  <label className="flex items-center gap-2 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={config.showValue ?? false}
                      onChange={(e) => updateConfig('showValue')(e.target.checked)}
                      className="rounded"
                    />
                    <span className="text-sm">{t('visualDashboard.showCurrentValue')}</span>
                  </label>

                  <label className="flex items-center gap-2 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={config.showThreshold ?? false}
                      onChange={(e) => updateConfig('showThreshold')(e.target.checked)}
                      className="rounded"
                    />
                    <span className="text-sm">{t('visualDashboard.showThreshold')}</span>
                  </label>

                  {config.showThreshold && (
                    <>
                      <Field>
                        <Label>{t('visualDashboard.threshold')}</Label>
                        <Input
                          type="number"
                          value={config.threshold ?? 20}
                          onChange={(e) => updateConfig('threshold')(Number(e.target.value))}
                          className="h-9"
                        />
                      </Field>

                      <ColorPicker
                        value={config.thresholdColor || '#ef4444'}
                        onChange={(color) => updateConfig('thresholdColor')(color)}
                        label={t('visualDashboard.thresholdColor')}
                        presets="semantic"
                      />
                    </>
                  )}
                </div>
              ),
            },
          ],
          displaySections: [],
          dataSourceSections: [
            {
              type: 'data-source' as const,
              props: {
                dataSource: config.dataSource,
                onChange: updateDataSource,
                allowedTypes: ['device-metric'],
              },
            },
          ],
        }

      case 'progress-bar':
        return {
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <Field>
                    <Label htmlFor="progress-bar-title">{t('visualDashboard.showTitle')}</Label>
                    <Input
                      id="progress-bar-title"
                      value={config.title as string || ''}
                      onChange={(e) => updateConfig('title')(e.target.value)}
                      placeholder={t('visualDashboard.enterTitle')}
                      className="h-10"
                    />
                  </Field>

                  <SelectField
                    label={t('visualDashboard.style')}
                    value={config.variant || 'default'}
                    onChange={updateConfig('variant')}
                    options={[
                      { value: 'default', label: t('visualDashboard.default') },
                      { value: 'compact', label: t('visualDashboard.compact') },
                      { value: 'circular', label: t('visualDashboard.horizontal') },
                    ]}
                  />

                  <ColorPicker
                    value={config.color || '#3b82f6'}
                    onChange={(color) => updateConfig('color')(color)}
                    label={t('visualDashboard.color')}
                    presets="primary"
                  />

                  <SelectField
                    label={t('visualDashboard.size')}
                    value={config.size || 'md'}
                    onChange={updateConfig('size')}
                    options={[
                      { value: 'sm', label: t('sizes.sm') },
                      { value: 'md', label: t('sizes.md') },
                      { value: 'lg', label: t('sizes.lg') },
                    ]}
                  />

                  <Field>
                    <Label>{t('visualDashboard.label')}</Label>
                    <Input
                      value={config.label || ''}
                      onChange={(e) => updateConfig('label')(e.target.value)}
                      placeholder={t('visualDashboard.descriptionPlaceholder')}
                      className="h-9"
                    />
                  </Field>

                  <div className="grid grid-cols-2 gap-3">
                    <Field>
                      <Label>警告阈值 (%)</Label>
                      <Input
                        type="number"
                        value={config.warningThreshold ?? 70}
                        onChange={(e) => updateConfig('warningThreshold')(Number(e.target.value))}
                        min={0}
                        max={100}
                        className="h-9"
                      />
                    </Field>
                    <Field>
                      <Label>危险阈值 (%)</Label>
                      <Input
                        type="number"
                        value={config.dangerThreshold ?? 90}
                        onChange={(e) => updateConfig('dangerThreshold')(Number(e.target.value))}
                        min={0}
                        max={100}
                        className="h-9"
                      />
                    </Field>
                  </div>
                  <p className="text-xs text-muted-foreground">
                    进度条颜色会根据阈值自动变化：正常 → 警告 → 危险
                  </p>

                  <Field>
                    <Label>最大值</Label>
                    <Input
                      type="number"
                      value={config.max ?? 100}
                      onChange={(e) => updateConfig('max')(Number(e.target.value))}
                      min={1}
                      className="h-9"
                    />
                  </Field>

                  <label className="flex items-center gap-2 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={config.showCard ?? true}
                      onChange={(e) => updateConfig('showCard')(e.target.checked)}
                      className="rounded"
                    />
                    <span className="text-sm">{t('visualDashboard.showTitle')}</span>
                  </label>
                </div>
              ),
            },
          ],
          displaySections: [
            // Data mapping configuration in display section
            {
              type: 'custom' as const,
              render: () => (
                <DataMappingConfig
                  dataMapping={config.dataMapping as SingleValueMappingConfig}
                  onChange={updateDataMapping}
                  mappingType="single"
                  label={t('visualDashboard.style')}
                  readonly={!config.dataSource}
                />
              ),
            },
          ],
          dataSourceSections: [
            {
              type: 'data-source' as const,
              props: {
                dataSource: config.dataSource,
                onChange: updateDataSource,
                allowedTypes: ['device-metric'],
              },
            },
          ],
        }

      case 'led-indicator':
        return {
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <Field>
                    <Label htmlFor="led-indicator-title">显示标题</Label>
                    <Input
                      id="led-indicator-title"
                      value={config.title as string || ''}
                      onChange={(e) => updateConfig('title')(e.target.value)}
                      placeholder={t('visualDashboard.enterTitle')}
                      className="h-10"
                    />
                  </Field>

                  <Field>
                    <Label>{t('visualDashboard.label')}</Label>
                    <Input
                      value={config.label || ''}
                      onChange={(e) => updateConfig('label')(e.target.value)}
                      placeholder={t('visualDashboard.descriptionPlaceholder')}
                      className="h-9"
                    />
                  </Field>

                  <SelectField
                    label={t('visualDashboard.size')}
                    value={config.size || 'md'}
                    onChange={updateConfig('size')}
                    options={[
                      { value: 'sm', label: t('sizes.sm') },
                      { value: 'md', label: t('sizes.md') },
                      { value: 'lg', label: t('sizes.lg') },
                    ]}
                  />

                  <ColorPicker
                    value={config.color || '#22c55e'}
                    onChange={(color) => updateConfig('color')(color)}
                    label={t('visualDashboard.color')}
                    presets="semantic"
                  />

                  <div className="flex items-center gap-6">
                    <div className="flex items-center gap-2">
                      <Checkbox
                        id="showGlow"
                        checked={config.showGlow ?? true}
                        onCheckedChange={(checked) => updateConfig('showGlow')(checked === true)}
                      />
                      <label htmlFor="showGlow" className="text-sm cursor-pointer">
                        发光效果
                      </label>
                    </div>

                    <div className="flex items-center gap-2">
                      <Checkbox
                        id="showAnimation"
                        checked={config.showAnimation ?? true}
                        onCheckedChange={(checked) => updateConfig('showAnimation')(checked === true)}
                      />
                      <label htmlFor="showAnimation" className="text-sm cursor-pointer">
                        动画效果
                      </label>
                    </div>

                    <div className="flex items-center gap-2">
                      <Checkbox
                        id="showCard"
                        checked={config.showCard ?? true}
                        onCheckedChange={(checked) => updateConfig('showCard')(checked === true)}
                      />
                      <label htmlFor="showCard" className="text-sm cursor-pointer">
                        显示卡片
                      </label>
                    </div>
                  </div>

                  <Field>
                    <Label>默认状态（无数据源时）</Label>
                    <Select
                      value={config.state || 'on'}
                      onValueChange={updateConfig('state')}
                    >
                      <SelectTrigger className="h-9">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="on">开启</SelectItem>
                        <SelectItem value="off">关闭</SelectItem>
                        <SelectItem value="error">错误</SelectItem>
                        <SelectItem value="warning">警告</SelectItem>
                        <SelectItem value="unknown">未知</SelectItem>
                      </SelectContent>
                    </Select>
                  </Field>

                  <Field>
                    <Label>默认状态</Label>
                    <Select
                      value={config.defaultState || 'unknown'}
                      onValueChange={updateConfig('defaultState')}
                    >
                      <SelectTrigger className="h-9">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="on">开启</SelectItem>
                        <SelectItem value="off">关闭</SelectItem>
                        <SelectItem value="error">错误</SelectItem>
                        <SelectItem value="warning">警告</SelectItem>
                        <SelectItem value="unknown">未知</SelectItem>
                      </SelectContent>
                    </Select>
                    <p className="text-xs text-muted-foreground mt-1">
                      当数据值不匹配任何映射规则时，显示此状态
                    </p>
                  </Field>

                  <div className="pt-2 border-t">
                    <div className="text-sm font-medium mb-3">字符串值映射</div>
                    <ValueMapEditor
                      valueMap={(config.valueMap || []).map((m: any) => ({
                        id: m.id || Date.now().toString() + Math.random(),
                        values: m.values || '',
                        pattern: m.pattern,
                        state: m.state || 'unknown',
                        label: m.label,
                        color: m.color,
                      }))}
                      onChange={(newValueMap) => {
                        updateConfig('valueMap')(newValueMap)
                      }}
                    />
                  </div>
                </div>
              ),
            },
          ],
          displaySections: [],
          dataSourceSections: [
            {
              type: 'data-source' as const,
              props: {
                dataSource: config.dataSource,
                onChange: updateDataSource,
                allowedTypes: ['device-metric'],
              },
            },
          ],
        }

      // ========== Charts ==========
      case 'line-chart':
        return {
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <Field>
                    <Label htmlFor="line-chart-title">显示标题</Label>
                    <Input
                      id="line-chart-title"
                      value={config.title as string || ''}
                      onChange={(e) => updateConfig('title')(e.target.value)}
                      placeholder={t('visualDashboard.enterTitle')}
                      className="h-10"
                    />
                  </Field>

                  <ColorPicker
                    value={config.color || '#3b82f6'}
                    onChange={(color) => updateConfig('color')(color)}
                    label="线条颜色"
                    presets="primary"
                  />

                  <SelectField
                    label={t('visualDashboard.size')}
                    value={config.size || 'md'}
                    onChange={updateConfig('size')}
                    options={[
                      { value: 'sm', label: t('sizes.sm') },
                      { value: 'md', label: t('sizes.md') },
                      { value: 'lg', label: t('sizes.lg') },
                    ]}
                  />

                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.smooth ?? true}
                        onChange={(e) => updateConfig('smooth')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">平滑曲线</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.fillArea ?? false}
                        onChange={(e) => updateConfig('fillArea')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">{t('visualDashboard.fillArea')}</span>
                    </label>
                  </div>

                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showGrid ?? true}
                        onChange={(e) => updateConfig('showGrid')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">显示网格</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showLegend ?? false}
                        onChange={(e) => updateConfig('showLegend')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">显示图例</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showTooltip ?? true}
                        onChange={(e) => updateConfig('showTooltip')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">显示提示</span>
                    </label>
                  </div>
                </div>
              ),
            },
          ],
          displaySections: [],
          dataSourceSections: [
            {
              type: 'data-source' as const,
              props: {
                dataSource: config.dataSource,
                onChange: updateDataSource,
                allowedTypes: ['device-metric'],
                multiple: true,
                maxSources: 5,
              },
            },
          ],
        }

      case 'area-chart':
        return {
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <Field>
                    <Label htmlFor="area-chart-title">显示标题</Label>
                    <Input
                      id="area-chart-title"
                      value={config.title as string || ''}
                      onChange={(e) => updateConfig('title')(e.target.value)}
                      placeholder={t('visualDashboard.enterTitle')}
                      className="h-10"
                    />
                  </Field>

                  <ColorPicker
                    value={config.color || '#3b82f6'}
                    onChange={(color) => updateConfig('color')(color)}
                    label="区域颜色"
                    presets="primary"
                  />

                  <SelectField
                    label={t('visualDashboard.size')}
                    value={config.size || 'md'}
                    onChange={updateConfig('size')}
                    options={[
                      { value: 'sm', label: t('sizes.sm') },
                      { value: 'md', label: t('sizes.md') },
                      { value: 'lg', label: t('sizes.lg') },
                    ]}
                  />

                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.smooth ?? true}
                        onChange={(e) => updateConfig('smooth')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">平滑曲线</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showGrid ?? true}
                        onChange={(e) => updateConfig('showGrid')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">显示网格</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showLegend ?? false}
                        onChange={(e) => updateConfig('showLegend')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">显示图例</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showTooltip ?? true}
                        onChange={(e) => updateConfig('showTooltip')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">显示提示</span>
                    </label>
                  </div>
                </div>
              ),
            },
          ],
          displaySections: [],
          dataSourceSections: [
            {
              type: 'data-source' as const,
              props: {
                dataSource: config.dataSource,
                onChange: updateDataSource,
                allowedTypes: ['device-metric'],
                multiple: true,
                maxSources: 5,
              },
            },
            {
              type: 'custom' as const,
              render: () => (
                <DataMappingConfig
                  dataMapping={config.dataMapping as TimeSeriesMappingConfig}
                  onChange={updateDataMapping}
                  mappingType="time-series"
                  label="数据映射配置"
                  readonly={false}
                />
              ),
            },
          ],
        }

      case 'bar-chart':
        return {
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <Field>
                    <Label htmlFor="bar-chart-title">显示标题</Label>
                    <Input
                      id="bar-chart-title"
                      value={config.title as string || ''}
                      onChange={(e) => updateConfig('title')(e.target.value)}
                      placeholder={t('visualDashboard.enterTitle')}
                      className="h-10"
                    />
                  </Field>

                  <ColorPicker
                    value={config.color || '#8b5cf6'}
                    onChange={(color) => updateConfig('color')(color)}
                    label="柱体颜色"
                    presets="primary"
                  />

                  <SelectField
                    label={t('visualDashboard.size')}
                    value={config.size || 'md'}
                    onChange={updateConfig('size')}
                    options={[
                      { value: 'sm', label: t('sizes.sm') },
                      { value: 'md', label: t('sizes.md') },
                      { value: 'lg', label: t('sizes.lg') },
                    ]}
                  />

                  <SelectField
                    label="布局"
                    value={config.layout || 'vertical'}
                    onChange={updateConfig('layout')}
                    options={[
                      { value: 'vertical', label: t('visualDashboard.vertical') },
                      { value: 'horizontal', label: '水平' },
                    ]}
                  />

                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.stacked ?? false}
                        onChange={(e) => updateConfig('stacked')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">堆叠</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showGrid ?? true}
                        onChange={(e) => updateConfig('showGrid')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">显示网格</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showLegend ?? false}
                        onChange={(e) => updateConfig('showLegend')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">显示图例</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showTooltip ?? true}
                        onChange={(e) => updateConfig('showTooltip')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">显示提示</span>
                    </label>
                  </div>
                </div>
              ),
            },
          ],
          displaySections: [],
          dataSourceSections: [
            {
              type: 'data-source' as const,
              props: {
                dataSource: config.dataSource,
                onChange: updateDataSource,
                allowedTypes: ['device-metric'],
                multiple: true,
                maxSources: 3,
              },
            },
          ],
        }

      case 'pie-chart':
        return {
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <Field>
                    <Label htmlFor="pie-chart-title">显示标题</Label>
                    <Input
                      id="pie-chart-title"
                      value={config.title as string || ''}
                      onChange={(e) => updateConfig('title')(e.target.value)}
                      placeholder={t('visualDashboard.enterTitle')}
                      className="h-10"
                    />
                  </Field>

                  <SelectField
                    label={t('visualDashboard.size')}
                    value={config.size || 'md'}
                    onChange={updateConfig('size')}
                    options={[
                      { value: 'sm', label: t('sizes.sm') },
                      { value: 'md', label: t('sizes.md') },
                      { value: 'lg', label: t('sizes.lg') },
                    ]}
                  />

                  <SelectField
                    label="类型"
                    value={config.variant || 'donut'}
                    onChange={updateConfig('variant')}
                    options={[
                      { value: 'pie', label: '饼图' },
                      { value: 'donut', label: '环形图' },
                    ]}
                  />

                  {config.variant === 'donut' && (
                    <Field>
                      <Label>内半径</Label>
                      <input
                        type="text"
                        value={config.innerRadius || '60%'}
                        onChange={(e) => updateConfig('innerRadius')(e.target.value)}
                        placeholder="60% or 60"
                        className="w-full h-9 px-3 rounded-md border border-input bg-background text-sm"
                      />
                    </Field>
                  )}

                  <Field>
                    <Label>外半径</Label>
                    <input
                      type="text"
                      value={config.outerRadius || '80%'}
                      onChange={(e) => updateConfig('outerRadius')(e.target.value)}
                      placeholder="80% or 80"
                      className="w-full h-9 px-3 rounded-md border border-input bg-background text-sm"
                    />
                  </Field>

                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showLegend ?? false}
                        onChange={(e) => updateConfig('showLegend')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">显示图例</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showTooltip ?? true}
                        onChange={(e) => updateConfig('showTooltip')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">显示提示</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showLabels ?? false}
                        onChange={(e) => updateConfig('showLabels')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">显示标签</span>
                    </label>
                  </div>
                </div>
              ),
            },
          ],
          displaySections: [],
          dataSourceSections: [
            {
              type: 'data-source' as const,
              props: {
                dataSource: config.dataSource,
                onChange: updateDataSource,
                allowedTypes: ['device-metric'],
              },
            },
          ],
        }

      // ========== Controls ==========
      case 'toggle-switch':
        return {
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <Field>
                    <Label htmlFor="toggle-switch-title">显示标题</Label>
                    <Input
                      id="toggle-switch-title"
                      value={config.title as string || ''}
                      onChange={(e) => updateConfig('title')(e.target.value)}
                      placeholder={t('visualDashboard.enterTitle')}
                      className="h-10"
                    />
                  </Field>

                  <Field>
                    <Label>{t('visualDashboard.label')}</Label>
                    <input
                      type="text"
                      value={config.label || ''}
                      onChange={(e) => updateConfig('label')(e.target.value)}
                      placeholder="如 主灯"
                      className="w-full h-9 px-3 rounded-md border border-input bg-background text-sm"
                    />
                  </Field>

                  <Field>
                    <Label>初始状态</Label>
                    <Select
                      value={config.initialState ? 'on' : 'off'}
                      onValueChange={(val) => updateConfig('initialState')(val === 'on')}
                    >
                      <SelectTrigger className="h-9">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="off">关闭</SelectItem>
                        <SelectItem value="on">开启</SelectItem>
                      </SelectContent>
                    </Select>
                    <p className="text-xs text-muted-foreground">显示状态，在收到命令响应前使用</p>
                  </Field>

                  <SelectField
                    label={t('visualDashboard.size')}
                    value={config.size || 'md'}
                    onChange={updateConfig('size')}
                    options={[
                      { value: 'sm', label: t('sizes.sm') },
                      { value: 'md', label: t('sizes.md') },
                      { value: 'lg', label: t('sizes.lg') },
                    ]}
                  />
                </div>
              ),
            },
          ],
          dataSourceSections: [
            {
              type: 'data-source' as const,
              props: {
                dataSource: config.dataSource,
                onChange: updateDataSource,
                allowedTypes: ['command'],
              },
            },
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <div className="p-3 rounded-lg bg-amber-500/10 border border-amber-500/20">
                    <p className="text-sm text-amber-700 dark:text-amber-300">
                      <strong>仅支持命令模式</strong><br />
                      此组件只能绑定到设备的命令接口，点击时发送开关命令。
                    </p>
                  </div>
                </div>
              ),
            },
          ],
        }

      // ========== Display & Content ==========
      case 'image-display':
        return {
          dataSourceSections: [
            {
              type: 'data-source' as const,
              props: {
                dataSource: config.dataSource,
                onChange: updateDataSource,
                allowedTypes: ['device-metric'],
              },
            },
          ],
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <Field>
                    <Label htmlFor="image-display-title">显示标题</Label>
                    <Input
                      id="image-display-title"
                      value={config.title as string || ''}
                      onChange={(e) => updateConfig('title')(e.target.value)}
                      placeholder={t('visualDashboard.enterTitle')}
                      className="h-10"
                    />
                  </Field>

                  <Field>
                    <Label>图片源</Label>
                    <Input
                      value={config.src || ''}
                      onChange={(e) => updateConfig('src')(e.target.value)}
                      placeholder="https://example.com/image.jpg 或 data:image/png;base64,..."
                      className="h-9"
                    />
                    <p className="text-xs text-muted-foreground mt-1">
                      支持 URL 或 Base64 (data:image/png;base64,...)
                    </p>
                  </Field>
                  <SelectField
                    label="适配模式"
                    value={config.fit || 'contain'}
                    onChange={updateConfig('fit')}
                    options={[
                      { value: 'contain', label: '包含' },
                      { value: 'cover', label: '覆盖' },
                      { value: 'fill', label: '填充' },
                      { value: 'none', label: '无' },
                      { value: 'scale-down', label: '缩小' },
                    ]}
                  />
                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2">
                      <input
                        type="checkbox"
                        checked={config.rounded ?? true}
                        onChange={(e) => updateConfig('rounded')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-xs">圆角</span>
                    </label>
                    <label className="flex items-center gap-2">
                      <input
                        type="checkbox"
                        checked={config.zoomable ?? true}
                        onChange={(e) => updateConfig('zoomable')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-xs">可缩放</span>
                    </label>
                    <label className="flex items-center gap-2">
                      <input
                        type="checkbox"
                        checked={config.showShadow ?? false}
                        onChange={(e) => updateConfig('showShadow')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-xs">阴影</span>
                    </label>
                  </div>
                </div>
              ),
            },
          ],
        }

      case 'image-history':
        return {
          dataSourceSections: [
            {
              type: 'data-source' as const,
              props: {
                dataSource: config.dataSource,
                onChange: updateDataSource,
                allowedTypes: ['device-metric'],
              },
            },
          ],
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <Field>
                    <Label htmlFor="image-history-title">显示标题</Label>
                    <Input
                      id="image-history-title"
                      value={config.title as string || ''}
                      onChange={(e) => updateConfig('title')(e.target.value)}
                      placeholder={t('visualDashboard.enterTitle')}
                      className="h-10"
                    />
                  </Field>

                  <SelectField
                    label="适配模式"
                    value={config.fit || 'contain'}
                    onChange={updateConfig('fit')}
                    options={[
                      { value: 'contain', label: '包含' },
                      { value: 'cover', label: '覆盖' },
                      { value: 'fill', label: '填充' },
                      { value: 'none', label: '无' },
                      { value: 'scale-down', label: '缩小' },
                    ]}
                  />
                  <div className="grid grid-cols-2 gap-3">
                    <Field>
                      <Label>最大图片数</Label>
                      <Input
                        type="number"
                        value={config.limit ?? 50}
                        onChange={(e) => updateConfig('limit')(Number(e.target.value))}
                        min={1}
                        max={200}
                        className="h-9"
                      />
                    </Field>
                    <Field>
                      <Label>时间范围（小时）</Label>
                      <Input
                        type="number"
                        value={config.timeRange ?? 1}
                        onChange={(e) => updateConfig('timeRange')(Number(e.target.value))}
                        min={1}
                        max={168}
                        step={1}
                        className="h-9"
                      />
                    </Field>
                  </div>
                  <div className="flex flex-wrap items-center gap-3">
                    <label className="flex items-center gap-2">
                      <input
                        type="checkbox"
                        checked={config.rounded ?? true}
                        onChange={(e) => updateConfig('rounded')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-xs">圆角</span>
                    </label>
                  </div>
                </div>
              ),
            },
          ],
        }

      case 'web-display':
        return {
          dataSourceSections: [
            {
              type: 'data-source' as const,
              props: {
                dataSource: config.dataSource,
                onChange: updateDataSource,
                allowedTypes: ['device-metric'],
              },
            },
          ],
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-4">
                  <Field>
                    <Label htmlFor="web-display-title">显示标题</Label>
                    <Input
                      id="web-display-title"
                      value={config.title as string || ''}
                      onChange={(e) => updateConfig('title')(e.target.value)}
                      placeholder={t('visualDashboard.enterTitle')}
                      className="h-10"
                    />
                  </Field>

                  <div className="space-y-2">
                    <label className="text-sm font-medium">Website URL</label>
                    <Input
                      value={config.src || ''}
                      onChange={(e) => updateConfig('src')(e.target.value)}
                      placeholder="https://example.com"
                      className="h-10"
                    />
                  </div>
                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2">
                      <input
                        type="checkbox"
                        checked={config.sandbox ?? true}
                        onChange={(e) => updateConfig('sandbox')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">沙盒隔离</span>
                    </label>
                    <label className="flex items-center gap-2">
                      <input
                        type="checkbox"
                        checked={config.showHeader ?? true}
                        onChange={(e) => updateConfig('showHeader')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">显示头部</span>
                    </label>
                  </div>
                </div>
              ),
            },
          ],
        }

      case 'markdown-display':
        return {
          dataSourceSections: [
            {
              type: 'data-source' as const,
              props: {
                dataSource: config.dataSource,
                onChange: updateDataSource,
                allowedTypes: ['device-metric'],
              },
            },
          ],
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <Field>
                    <Label htmlFor="markdown-display-title">显示标题</Label>
                    <Input
                      id="markdown-display-title"
                      value={config.title as string || ''}
                      onChange={(e) => updateConfig('title')(e.target.value)}
                      placeholder={t('visualDashboard.enterTitle')}
                      className="h-10"
                    />
                  </Field>

                  <Field>
                    <Label>Markdown 内容</Label>
                    <textarea
                      value={config.content || ''}
                      onChange={(e) => updateConfig('content')(e.target.value)}
                      placeholder="# 标题\n\n**粗体** 和 *斜体* 文本"
                      rows={6}
                      className="w-full px-3 py-2 rounded-md border border-input bg-background text-sm"
                    />
                  </Field>
                  <SelectField
                    label={t('visualDashboard.style')}
                    value={config.variant || 'default'}
                    onChange={updateConfig('variant')}
                    options={[
                      { value: 'default', label: '默认' },
                      { value: 'compact', label: t('visualDashboard.compact') },
                      { value: 'minimal', label: t('visualDashboard.minimal') },
                    ]}
                  />
                </div>
              ),
            },
          ],
        }

      case 'video-display':
        return {
          dataSourceSections: [
            {
              type: 'data-source' as const,
              props: {
                dataSource: config.dataSource,
                onChange: updateDataSource,
                allowedTypes: ['device', 'device-info'],
              },
            },
          ],
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <Field>
                    <Label htmlFor="video-display-title">显示标题</Label>
                    <Input
                      id="video-display-title"
                      value={config.title as string || ''}
                      onChange={(e) => updateConfig('title')(e.target.value)}
                      placeholder={t('visualDashboard.enterTitle')}
                      className="h-10"
                    />
                  </Field>

                  <Field>
                    <Label htmlFor="video-display-src">视频源</Label>
                    <Input
                      id="video-display-src"
                      value={config.src || ''}
                      onChange={(e) => updateConfig('src')(e.target.value)}
                      placeholder="https://example.com/video.mp4 或 rtsp://..."
                      className="h-9"
                    />
                    <p className="text-xs text-muted-foreground mt-1">
                      支持 MP4、WebM、HLS (.m3u8)、RTSP、RTMP 等格式
                    </p>
                  </Field>

                  <SelectField
                    label="视频类型"
                    value={config.type || 'file'}
                    onChange={updateConfig('type')}
                    options={[
                      { value: 'file', label: '视频文件/URL (MP4, WebM)' },
                      { value: 'stream', label: '视频流' },
                      { value: 'rtsp', label: 'RTSP' },
                      { value: 'rtmp', label: 'RTMP' },
                      { value: 'hls', label: 'HLS' },
                      { value: 'webrtc', label: 'WebRTC' },
                      { value: 'device-camera', label: '设备摄像头' },
                    ]}
                  />

                  <SelectField
                    label="适配方式"
                    value={config.fit || 'contain'}
                    onChange={updateConfig('fit')}
                    options={[
                      { value: 'contain', label: '包含 (完整显示)' },
                      { value: 'cover', label: '覆盖 (填充显示)' },
                      { value: 'fill', label: '拉伸 (填充)' },
                    ]}
                  />

                  <div className="grid grid-cols-2 gap-3">
                    <Field>
                      <Label>自动播放</Label>
                      <select
                        value={String(config.autoplay ?? false)}
                        onChange={(e) => updateConfig('autoplay')(e.target.value === 'true')}
                        className="w-full h-9 px-2 rounded-md border border-input bg-background text-sm"
                      >
                        <option value="false">关闭</option>
                        <option value="true">开启</option>
                      </select>
                    </Field>
                    <Field>
                      <Label>静音</Label>
                      <select
                        value={String(config.muted ?? true)}
                        onChange={(e) => updateConfig('muted')(e.target.value === 'true')}
                        className="w-full h-9 px-2 rounded-md border border-input bg-background text-sm"
                      >
                        <option value="true">静音</option>
                        <option value="false">开启声音</option>
                      </select>
                    </Field>
                  </div>

                  <div className="grid grid-cols-2 gap-3">
                    <Field>
                      <Label>显示控制条</Label>
                      <select
                        value={String(config.controls ?? true)}
                        onChange={(e) => updateConfig('controls')(e.target.value === 'true')}
                        className="w-full h-9 px-2 rounded-md border border-input bg-background text-sm"
                      >
                        <option value="true">显示</option>
                        <option value="false">隐藏</option>
                      </select>
                    </Field>
                    <Field>
                      <Label>循环播放</Label>
                      <select
                        value={String(config.loop ?? false)}
                        onChange={(e) => updateConfig('loop')(e.target.value === 'true')}
                        className="w-full h-9 px-2 rounded-md border border-input bg-background text-sm"
                      >
                        <option value="false">关闭</option>
                        <option value="true">开启</option>
                      </select>
                    </Field>
                  </div>

                  <Field>
                    <Label>全屏按钮</Label>
                    <select
                      value={String(config.showFullscreen ?? true)}
                      onChange={(e) => updateConfig('showFullscreen')(e.target.value === 'true')}
                      className="w-full h-9 px-2 rounded-md border border-input bg-background text-sm"
                    >
                      <option value="true">显示</option>
                      <option value="false">隐藏</option>
                    </select>
                  </Field>
                </div>
              ),
            },
          ],
        }

      case 'map-display':
        return {
          dataSourceSections: [
            {
              type: 'data-source' as const,
              props: {
                dataSource: config.dataSource,
                onChange: (newSource: DataSourceOrList | DataSource | undefined) => {
                  console.log('=== DataSource onChange ===')
                  console.log('newSource:', newSource)

                  updateDataSource(newSource)
                  // When data source changes, update bindings automatically
                  if (Array.isArray(newSource) && newSource.length > 0) {
                    const newBindings: MapBinding[] = newSource.map((ds, index) => {
                      console.log(`=== Processing dataSource[${index}] ===`)
                      console.log('  ds:', JSON.stringify(ds))

                      // Determine type based on dataSource type
                      let bindingType: MapBindingType = 'device'
                      if (ds.type === 'metric' || ds.type === 'telemetry') bindingType = 'metric'
                      else if (ds.type === 'command') bindingType = 'command'

                      console.log(`  ds.type="${ds.type}" -> bindingType="${bindingType}"`)

                      // Use metricId for metrics/telemetry, deviceId for devices/commands
                      const identifier = (ds.type === 'metric' || ds.type === 'telemetry')
                        ? (ds.metricId || ds.property || 'unknown')
                        : (ds.deviceId || ds.command || 'unknown')

                      console.log(`  identifier="${identifier}"`)

                      // Look for existing binding - match by dataSource content
                      // We match regardless of type to allow type corrections
                      const existingBinding = (config.bindings as MapBinding[])?.find(b => {
                        if (!b.dataSource) return false
                        const bDs = b.dataSource as any

                        // Match by deviceId+metricId/property for metric/telemetry
                        if (bindingType === 'metric' || ds.type === 'telemetry') {
                          return (bDs.deviceId === ds.deviceId) && (
                            bDs.metricId === ds.metricId ||
                            bDs.property === ds.metricId ||
                            bDs.property === ds.property
                          )
                        }
                        // Match by deviceId+command for command
                        if (bindingType === 'command') {
                          return (bDs.deviceId === ds.deviceId) && (bDs.command === ds.command)
                        }
                        // Match by deviceId for device
                        return bDs.deviceId === ds.deviceId && !ds.metricId && !ds.property && !ds.command
                      })

                      // Create or update binding - update type if changed
                      // Generate unique ID: type-deviceId-metricId/command or type-deviceId-index
                      const generateBindingId = () => {
                        if (ds.type === 'metric' || ds.type === 'telemetry') {
                          return `${bindingType}-${ds.deviceId}-${ds.metricId || ds.property || index}`
                        } else if (ds.type === 'command') {
                          return `${bindingType}-${ds.deviceId}-${ds.command}`
                        } else {
                          return `${bindingType}-${ds.deviceId}-${index}`
                        }
                      }

                      const baseBinding = existingBinding || {
                        id: generateBindingId(),
                        position: { lat: 39.9042, lng: 116.4074 },
                      }

                      const newBinding = {
                        ...baseBinding,
                        id: existingBinding?.id || generateBindingId(), // Preserve existing ID if available
                        type: bindingType,
                        icon: bindingType,
                        name: (ds.type === 'metric' || ds.type === 'telemetry')
                          ? (ds.metricId || ds.property || `指标${index + 1}`)
                          : ds.type === 'command'
                            ? `${ds.deviceId || ''} → ${ds.command || ''}`
                            : (ds.deviceId || `设备${index + 1}`),
                        dataSource: ds,
                        // Preserve position if existing
                        position: existingBinding?.position || baseBinding.position,
                      }

                      console.log(`  -> Created binding:`, newBinding)
                      return newBinding
                    })

                    console.log('Final newBindings:', newBindings)
                    updateConfig('bindings')(newBindings)
                  }
                },
                allowedTypes: ['device', 'metric', 'command'],
                multiple: true,
                maxSources: 50,
              },
            },
          ],
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <div className="grid grid-cols-2 gap-3">
                    <Field>
                      <Label>纬度</Label>
                      <Input
                        type="number"
                        step="0.0001"
                        value={(config.center as { lat: number } | undefined)?.lat ?? 39.9042}
                        onChange={(e) => updateConfig('center')({ ...(config.center as { lat: number; lng: number } | undefined) || { lat: 39.9042, lng: 116.4074 }, lat: parseFloat(e.target.value) })}
                        placeholder="39.9042"
                        className="h-9"
                      />
                    </Field>
                    <Field>
                      <Label>经度</Label>
                      <Input
                        type="number"
                        step="0.0001"
                        value={(config.center as { lng: number } | undefined)?.lng ?? 116.4074}
                        onChange={(e) => updateConfig('center')({ ...(config.center as { lat: number; lng: number } | undefined) || { lat: 39.9042, lng: 116.4074 }, lng: parseFloat(e.target.value) })}
                        placeholder="116.4074"
                        className="h-9"
                      />
                    </Field>
                  </div>

                  <div className="grid grid-cols-3 gap-3">
                    <Field>
                      <Label>缩放级别</Label>
                      <Input
                        type="number"
                        min={config.minZoom ?? 2}
                        max={config.maxZoom ?? 18}
                        value={config.zoom ?? 10}
                        onChange={(e) => updateConfig('zoom')(parseFloat(e.target.value))}
                        className="h-9"
                      />
                    </Field>
                    <Field>
                      <Label>最小缩放</Label>
                      <Input
                        type="number"
                        min={1}
                        max={10}
                        value={config.minZoom ?? 2}
                        onChange={(e) => updateConfig('minZoom')(parseFloat(e.target.value))}
                        className="h-9"
                      />
                    </Field>
                    <Field>
                      <Label>最大缩放</Label>
                      <Input
                        type="number"
                        min={10}
                        max={20}
                        value={config.maxZoom ?? 18}
                        onChange={(e) => updateConfig('maxZoom')(parseFloat(e.target.value))}
                        className="h-9"
                      />
                    </Field>
                  </div>

                  <SelectField
                    label="地图图层"
                    value={config.tileLayer || 'osm'}
                    onChange={updateConfig('tileLayer')}
                    options={[
                      { value: 'osm', label: 'OpenStreetMap' },
                      { value: 'satellite', label: '卫星图' },
                      { value: 'dark', label: '暗色模式' },
                      { value: 'terrain', label: '地形图' },
                    ]}
                  />

                  <Field>
                    <Label>标记点颜色</Label>
                    <Input
                      type="color"
                      value={config.markerColor || '#3b82f6'}
                      onChange={(e) => updateConfig('markerColor')(e.target.value)}
                      className="h-9 w-full"
                    />
                  </Field>

                  <div className="grid grid-cols-2 gap-3">
                    <Field>
                      <Label>显示控制栏</Label>
                      <select
                        value={String(config.showControls ?? true)}
                        onChange={(e) => updateConfig('showControls')(e.target.value === 'true')}
                        className="w-full h-9 px-2 rounded-md border border-input bg-background text-sm"
                      >
                        <option value="true">显示</option>
                        <option value="false">隐藏</option>
                      </select>
                    </Field>
                    <Field>
                      <Label>显示图层</Label>
                      <select
                        value={String(config.showLayers ?? true)}
                        onChange={(e) => updateConfig('showLayers')(e.target.value === 'true')}
                        className="w-full h-9 px-2 rounded-md border border-input bg-background text-sm"
                      >
                        <option value="true">显示</option>
                        <option value="false">隐藏</option>
                      </select>
                    </Field>
                  </div>

                  <div className="grid grid-cols-2 gap-3">
                    <Field>
                      <Label>可交互</Label>
                      <select
                        value={String(config.interactive ?? true)}
                        onChange={(e) => updateConfig('interactive')(e.target.value === 'true')}
                        className="w-full h-9 px-2 rounded-md border border-input bg-background text-sm"
                      >
                        <option value="true">是</option>
                        <option value="false">否</option>
                      </select>
                    </Field>
                    <Field>
                      <Label>全屏按钮</Label>
                      <select
                        value={String(config.showFullscreen ?? true)}
                        onChange={(e) => updateConfig('showFullscreen')(e.target.value === 'true')}
                        className="w-full h-9 px-2 rounded-md border border-input bg-background text-sm"
                      >
                        <option value="true">显示</option>
                        <option value="false">隐藏</option>
                      </select>
                    </Field>
                  </div>
                </div>
              ),
            },
          ],
          displaySections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-4">
                  <Field>
                    <Label htmlFor="map-display-title">显示标题</Label>
                    <Input
                      id="map-display-title"
                      value={config.title as string || ''}
                      onChange={(e) => updateConfig('title')(e.target.value)}
                      placeholder={t('visualDashboard.enterTitle')}
                      className="h-10"
                    />
                  </Field>

                  <div className="flex items-center justify-between">
                    <div>
                      <h3 className="text-sm font-medium">标记绑定</h3>
                      <p className="text-xs text-muted-foreground mt-1">
                        管理地图上的设备、指标、指令标记
                      </p>
                    </div>
                    <Button
                      type="button"
                      variant="default"
                      size="sm"
                      onClick={() => {
                        // Get the latest bindings from the store, not just local state
                        const latestDashboard = useStore.getState().currentDashboard
                        const latestComponent = latestDashboard?.components.find(c => c.id === selectedComponent?.id)
                        let latestBindings = (latestComponent as any)?.config?.bindings as MapBinding[] || []

                        // Fix duplicate IDs - regenerate IDs for bindings with duplicate IDs
                        const idCount = new Map<string, number>()
                        latestBindings = latestBindings.map((binding, index) => {
                          const ds = binding.dataSource as any
                          const currentId = binding.id

                          // Check if this ID is duplicated
                          idCount.set(currentId, (idCount.get(currentId) || 0) + 1)

                          // If ID will be duplicated or uses old format, regenerate it
                          if (idCount.get(currentId)! > 1 || binding.type === 'device' && ds?.metricId) {
                            // Generate unique ID based on type and data
                            let newId: string
                            if (binding.type === 'metric' || ds?.type === 'telemetry') {
                              newId = `metric-${ds?.deviceId}-${ds?.metricId || ds?.property || index}`
                            } else if (binding.type === 'command') {
                              newId = `command-${ds?.deviceId}-${ds?.command}`
                            } else {
                              newId = `device-${ds?.deviceId}-${index}`
                            }
                            console.log(`Fixing duplicate ID: ${currentId} -> ${newId}`)
                            return { ...binding, id: newId }
                          }

                          return binding
                        })

                        console.log('Opening map editor with bindings from store:', latestBindings)
                        setMapEditorBindings(latestBindings)
                        setMapEditorOpen(true)
                      }}
                    >
                      <MapIcon className="h-4 w-4 mr-1" />
                      打开地图编辑器
                    </Button>
                  </div>

                  {/* Bindings List - Grouped by Type */}
                  <div className="border rounded-lg overflow-hidden">
                    {(() => {
                      // Get the latest bindings from the store for display
                      const latestDashboard = useStore.getState().currentDashboard
                      const latestComponent = latestDashboard?.components.find(c => c.id === selectedComponent?.id)
                      let displayBindings = (latestComponent as any)?.config?.bindings as MapBinding[] || []

                      // Fix duplicate IDs for display and interaction
                      const idCount = new Map<string, number>()
                      displayBindings = displayBindings.map((binding, index) => {
                        const ds = binding.dataSource as any
                        const currentId = binding.id
                        idCount.set(currentId, (idCount.get(currentId) || 0) + 1)

                        // If ID is duplicated or binding type is wrong (e.g., telemetry marked as device)
                        if (idCount.get(currentId)! > 1 || (binding.type === 'device' && ds?.type === 'telemetry')) {
                          let newId: string
                          let newType = binding.type

                          // Fix type for telemetry bindings
                          if (ds?.type === 'telemetry' || ds?.type === 'metric') {
                            newType = 'metric'
                          }

                          if (newType === 'metric' || ds?.type === 'telemetry') {
                            newId = `metric-${ds?.deviceId}-${ds?.metricId || ds?.property || index}`
                          } else if (newType === 'command') {
                            newId = `command-${ds?.deviceId}-${ds?.command}`
                          } else {
                            newId = `device-${ds?.deviceId}-${index}`
                          }
                          console.log(`Fixing binding: ${currentId} -> ${newId}, type: ${binding.type} -> ${newType}`)
                          return { ...binding, id: newId, type: newType as any, icon: newType as any }
                        }
                        return binding
                      })

                      // Debug logging for bindings
                      console.log('=== Display Bindings Debug ===')
                      console.log('displayBindings count:', displayBindings.length)
                      displayBindings.forEach((b, i) => {
                        console.log(`  [${i}] id="${b.id}" type="${b.type}" name="${b.name}"`)
                      })

                      // Group by type
                      const groupedBindings = {
                        device: displayBindings.filter(b => b.type === 'device'),
                        metric: displayBindings.filter(b => b.type === 'metric'),
                        command: displayBindings.filter(b => b.type === 'command'),
                        marker: displayBindings.filter(b => b.type === 'marker'),
                      }

                      console.log('Grouped counts:', {
                        device: groupedBindings.device.length,
                        metric: groupedBindings.metric.length,
                        command: groupedBindings.command.length,
                        marker: groupedBindings.marker.length,
                      })

                      const TYPE_CONFIG = {
                        device: {
                          label: '设备',
                          color: 'bg-green-500',
                          textColor: 'text-green-600',
                          bgColor: 'bg-green-50 dark:bg-green-950/30',
                          borderColor: 'border-green-200 dark:border-green-800',
                          icon: MapPin,
                          description: '显示设备位置和状态'
                        },
                        metric: {
                          label: '指标',
                          color: 'bg-purple-500',
                          textColor: 'text-purple-600',
                          bgColor: 'bg-purple-50 dark:bg-purple-950/30',
                          borderColor: 'border-purple-200 dark:border-purple-800',
                          icon: Activity,
                          description: '显示指标数值和趋势'
                        },
                        command: {
                          label: '指令',
                          color: 'bg-blue-500',
                          textColor: 'text-blue-600',
                          bgColor: 'bg-blue-50 dark:bg-blue-950/30',
                          borderColor: 'border-blue-200 dark:border-blue-800',
                          icon: Zap,
                          description: '快速执行设备指令'
                        },
                        marker: {
                          label: '位置标记',
                          color: 'bg-orange-500',
                          textColor: 'text-orange-600',
                          bgColor: 'bg-orange-50 dark:bg-orange-950/30',
                          borderColor: 'border-orange-200 dark:border-orange-800',
                          icon: Monitor,
                          description: '自定义位置标记'
                        },
                      } as const

                      if (displayBindings.length === 0) {
                        return (
                          <div className="p-6 text-center text-muted-foreground">
                            <MapIcon className="h-8 w-8 mx-auto mb-2 opacity-50" />
                            <p className="text-sm">暂无标记绑定</p>
                            <p className="text-xs mt-1">请先在上方添加数据源，然后点击"打开地图编辑器"设置位置</p>
                          </div>
                        )
                      }

                      return (Object.keys(groupedBindings) as Array<keyof typeof groupedBindings>).map(type => {
                        const typeBindings = groupedBindings[type]
                        if (typeBindings.length === 0) return null

                        const config = TYPE_CONFIG[type]
                        const Icon = config.icon

                        return (
                          <div key={type} className="border-b last:border-b-0">
                            {/* Type Header */}
                            <div className={`px-3 py-2 ${config.bgColor} border-b ${config.borderColor} flex items-center justify-between`}>
                              <div className="flex items-center gap-2">
                                <div className={`w-5 h-5 rounded-full ${config.color} flex items-center justify-center`}>
                                  <Icon className="h-3 w-3 text-white" />
                                </div>
                                <span className="text-sm font-medium">{config.label}</span>
                                <span className="text-xs text-muted-foreground">({typeBindings.length})</span>
                              </div>
                              <span className="text-xs text-muted-foreground">{config.description}</span>
                            </div>

                            {/* Bindings of this type */}
                            <div className="divide-y">
                              {typeBindings.map((binding) => {
                                const positionText = binding.position && binding.position !== 'auto'
                                  ? `(${binding.position.lat.toFixed(4)}, ${binding.position.lng.toFixed(4)})`
                                  : '自动定位'

                                // Get device/metric info from dataSource
                                const deviceId = (binding.dataSource as any)?.deviceId
                                const metricId = (binding.dataSource as any)?.metricId
                                const command = (binding.dataSource as any)?.command

                                return (
                                  <div
                                    key={binding.id}
                                    className={`flex items-center gap-3 p-3 hover:bg-muted/50 transition-colors cursor-pointer`}
                                    onClick={() => {
                                      // Different interactions based on type
                                      if (type === 'device') {
                                        // Show device details
                                        console.log('Device clicked:', deviceId)
                                        // TODO: Open device details panel
                                      } else if (type === 'metric') {
                                        // Show metric value/trend
                                        console.log('Metric clicked:', deviceId, metricId)
                                        // TODO: Show metric tooltip
                                      } else if (type === 'command') {
                                        // Execute command
                                        console.log('Command clicked:', deviceId, command)
                                        // TODO: Execute command
                                      }
                                    }}
                                  >
                                    <div className={`w-8 h-8 rounded-full flex items-center justify-center ${config.color}/20 ${config.textColor}`}>
                                      <Icon className="h-4 w-4" />
                                    </div>
                                    <div className="flex-1 min-w-0">
                                      <div className="text-sm font-medium truncate">{binding.name}</div>
                                      <div className="flex items-center gap-2 text-xs text-muted-foreground">
                                        <span>{positionText}</span>
                                        {deviceId && <span>• {deviceId.slice(0, 8)}...</span>}
                                        {metricId && <span>• {metricId}</span>}
                                        {command && <span>• {command}</span>}
                                      </div>
                                    </div>
                                    <div className="text-xs text-muted-foreground">
                                      {type === 'device' && <span className="text-blue-500">查看详情</span>}
                                      {type === 'metric' && <span className="text-green-500">查看数值</span>}
                                      {type === 'command' && <span className="text-orange-500">执行</span>}
                                    </div>
                                  </div>
                                )
                              })}
                            </div>
                          </div>
                        )
                      })
                    })()}
                  </div>

                  {/* Legend */}
                  <div className="flex items-center gap-4 text-xs text-muted-foreground">
                    <div className="flex items-center gap-1">
                      <div className="w-3 h-3 rounded-full bg-blue-500"></div>
                      <span>设备</span>
                    </div>
                    <div className="flex items-center gap-1">
                      <div className="w-3 h-3 rounded-full bg-green-500"></div>
                      <span>指标</span>
                    </div>
                    <div className="flex items-center gap-1">
                      <div className="w-3 h-3 rounded-full bg-orange-500"></div>
                      <span>指令</span>
                    </div>
                    <div className="flex items-center gap-1">
                      <div className="w-3 h-3 rounded-full bg-purple-500"></div>
                      <span>位置标记</span>
                    </div>
                  </div>
                </div>
              ),
            },
          ],
        }

      case 'custom-layer':
        return {
          dataSourceSections: [
            {
              type: 'data-source' as const,
              props: {
                dataSource: config.bindings as any,
                onChange: (newDataSources: DataSourceOrList | undefined) => {
                  // Convert dataSources to LayerBinding format
                  // Handle both single object and array
                  const sourcesArray = newDataSources
                    ? Array.isArray(newDataSources)
                      ? newDataSources
                      : [newDataSources]
                    : []

                  const newBindings = sourcesArray.map((ds: any, index: number) => {
                    // Determine type based on dataSource type
                    let bindingType: LayerBindingType = 'device'
                    if (ds.type === 'metric' || ds.type === 'telemetry') bindingType = 'metric'
                    else if (ds.type === 'command') bindingType = 'command'

                    // Look for existing binding
                    const existingBinding = (config.bindings as LayerBinding[])?.find(b => {
                      if (!b.dataSource) return false
                      const bDs = b.dataSource as any
                      return bDs.deviceId === ds.deviceId &&
                        bDs.metricId === ds.metricId &&
                        bDs.property === ds.property &&
                        bDs.command === ds.command
                    })

                    // Generate unique ID
                    const generateBindingId = () => {
                      if (ds.type === 'metric' || ds.type === 'telemetry') {
                        return `${bindingType}-${ds.deviceId}-${ds.metricId || ds.property || index}`
                      } else if (ds.type === 'command') {
                        return `${bindingType}-${ds.deviceId}-${ds.command}`
                      } else {
                        return `${bindingType}-${ds.deviceId}-${index}`
                      }
                    }

                    const baseBinding = existingBinding || {
                      id: generateBindingId(),
                      position: { x: 50, y: 50 },
                    }

                    return {
                      ...baseBinding,
                      id: existingBinding?.id || generateBindingId(),
                      type: bindingType,
                      icon: bindingType,
                      name: (ds.type === 'metric' || ds.type === 'telemetry')
                        ? (ds.metricId || ds.property || `指标${index + 1}`)
                        : ds.type === 'command'
                          ? `${ds.deviceId || ''} → ${ds.command || ''}`
                          : (ds.deviceId || `设备${index + 1}`),
                      dataSource: ds,
                      position: existingBinding?.position || baseBinding.position,
                    } as LayerBinding
                  })

                  // Preserve existing text/icon bindings that aren't in the data sources
                  const existingTextIconBindings = (config.bindings as LayerBinding[])?.filter(b => {
                    if (b.type === 'text' || b.type === 'icon') return true
                    // Also check if this binding is from a dataSource that's no longer present
                    const ds = b.dataSource as any
                    if (ds && ds.deviceId) {
                      return !sourcesArray.some((s: any) => s.deviceId === ds.deviceId)
                    }
                    return false
                  }) || []

                  updateConfig('bindings')([...newBindings, ...existingTextIconBindings])
                },
                allowedTypes: ['device', 'metric', 'command'],
                multiple: true,
                maxSources: 20,
              },
            },
          ],
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-3">
                  <SelectField
                    label="背景类型"
                    value={config.backgroundType || 'grid'}
                    onChange={updateConfig('backgroundType')}
                    options={[
                      { value: 'grid', label: '网格' },
                      { value: 'color', label: '纯色' },
                      { value: 'image', label: '图片' },
                      { value: 'transparent', label: '透明' },
                    ]}
                  />

                  {config.backgroundType === 'color' && (
                    <Field>
                      <Label>背景颜色</Label>
                      <Input
                        type="color"
                        value={config.backgroundColor || '#f0f0f0'}
                        onChange={(e) => updateConfig('backgroundColor')(e.target.value)}
                        className="h-9 w-full"
                      />
                    </Field>
                  )}

                  {config.backgroundType === 'image' && (
                    <>
                      <Field>
                        <Label>背景图片 URL</Label>
                        <Input
                          value={config.backgroundImage || ''}
                          onChange={(e) => updateConfig('backgroundImage')(e.target.value)}
                          placeholder="https://example.com/image.png"
                          className="h-9"
                        />
                      </Field>
                      <Field>
                        <Label>或上传图片</Label>
                        <div className="flex items-center gap-2">
                          <Input
                            type="file"
                            accept="image/*"
                            onChange={(e) => {
                              const file = e.target.files?.[0]
                              if (file) {
                                const reader = new FileReader()
                                reader.onload = (e) => {
                                  updateConfig('backgroundImage')(e.target?.result as string)
                                }
                                reader.readAsDataURL(file)
                              }
                            }}
                            className="h-9"
                          />
                          {config.backgroundImage && (
                            <Button
                              type="button"
                              variant="outline"
                              size="sm"
                              onClick={() => updateConfig('backgroundImage')('')}
                            >
                              清除
                            </Button>
                          )}
                        </div>
                      </Field>
                      {config.backgroundImage && (
                        <div className="mt-2">
                          <Label className="text-xs text-muted-foreground">预览</Label>
                          <div
                            className="w-full h-24 bg-muted rounded-md bg-cover bg-center border"
                            style={{ backgroundImage: `url(${config.backgroundImage})` }}
                          />
                        </div>
                      )}
                    </>
                  )}

                  {config.backgroundType === 'grid' && (
                    <Field>
                      <Label>网格大小</Label>
                      <Input
                        type="number"
                        min={10}
                        max={50}
                        value={config.gridSize ?? 20}
                        onChange={(e) => updateConfig('gridSize')(Number(e.target.value))}
                        className="h-9"
                      />
                    </Field>
                  )}

                  <div className="grid grid-cols-2 gap-3">
                    <Field>
                      <Label>显示控制栏</Label>
                      <select
                        value={String(config.showControls ?? true)}
                        onChange={(e) => updateConfig('showControls')(e.target.value === 'true')}
                        className="w-full h-9 px-2 rounded-md border border-input bg-background text-sm"
                      >
                        <option value="true">显示</option>
                        <option value="false">隐藏</option>
                      </select>
                    </Field>
                    <Field>
                      <Label>显示全屏按钮</Label>
                      <select
                        value={String(config.showFullscreen ?? true)}
                        onChange={(e) => updateConfig('showFullscreen')(e.target.value === 'true')}
                        className="w-full h-9 px-2 rounded-md border border-input bg-background text-sm"
                      >
                        <option value="true">显示</option>
                        <option value="false">隐藏</option>
                      </select>
                    </Field>
                  </div>
                </div>
              ),
            },
          ],
          displaySections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-4">
                  <div className="flex items-center justify-between">
                    <div>
                      <h3 className="text-sm font-medium">图层项绑定</h3>
                      <p className="text-xs text-muted-foreground mt-1">
                        管理图层上的设备、指标、指令等项
                      </p>
                    </div>
                    <div className="flex items-center gap-2">
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        onClick={() => {
                          const latestDashboard = useStore.getState().currentDashboard
                          const latestComponent = latestDashboard?.components.find(c => c.id === selectedComponent?.id)
                          let latestBindings = (latestComponent as any)?.config?.bindings as LayerBinding[] || []

                          // Create new text binding
                          const newBinding: LayerBinding = {
                            id: `text-${Date.now()}`,
                            type: 'text',
                            icon: 'text',
                            name: '新文本',
                            position: { x: 50, y: 50 },
                            dataSource: { type: 'static', text: '文本内容' },
                          }

                          updateConfig('bindings')([...latestBindings, newBinding])
                          setLayerEditorBindings([...latestBindings, newBinding])
                          setLayerEditorOpen(true)
                        }}
                      >
                        <Type className="h-4 w-4 mr-1" />
                        添加文本
                      </Button>
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        onClick={() => {
                          const latestDashboard = useStore.getState().currentDashboard
                          const latestComponent = latestDashboard?.components.find(c => c.id === selectedComponent?.id)
                          let latestBindings = (latestComponent as any)?.config?.bindings as LayerBinding[] || []

                          // Create new icon binding
                          const newBinding: LayerBinding = {
                            id: `icon-${Date.now()}`,
                            type: 'icon',
                            icon: 'icon',
                            name: '新图标',
                            position: { x: 50, y: 50 },
                            dataSource: { type: 'static', icon: '⭐' },
                          }

                          updateConfig('bindings')([...latestBindings, newBinding])
                          setLayerEditorBindings([...latestBindings, newBinding])
                          setLayerEditorOpen(true)
                        }}
                      >
                        <Sparkles className="h-4 w-4 mr-1" />
                        添加图标
                      </Button>
                      <Button
                        type="button"
                        variant="default"
                        size="sm"
                        onClick={() => {
                          const latestDashboard = useStore.getState().currentDashboard
                          const latestComponent = latestDashboard?.components.find(c => c.id === selectedComponent?.id)
                          let latestBindings = (latestComponent as any)?.config?.bindings as LayerBinding[] || []
                          setLayerEditorBindings(latestBindings)
                          setLayerEditorOpen(true)
                        }}
                      >
                        <Layers className="h-4 w-4 mr-1" />
                        打开图层编辑器
                      </Button>
                    </div>
                  </div>

                  {/* Bindings List - Grouped by Type */}
                  <div className="border rounded-lg overflow-hidden">
                    {(() => {
                      const latestDashboard = useStore.getState().currentDashboard
                      const latestComponent = latestDashboard?.components.find(c => c.id === selectedComponent?.id)
                      let displayBindings = (latestComponent as any)?.config?.bindings as LayerBinding[] || []

                      // Group by type
                      const groupedBindings = {
                        device: displayBindings.filter(b => b.type === 'device'),
                        metric: displayBindings.filter(b => b.type === 'metric'),
                        command: displayBindings.filter(b => b.type === 'command'),
                        text: displayBindings.filter(b => b.type === 'text'),
                        icon: displayBindings.filter(b => b.type === 'icon'),
                      }

                      const LAYER_TYPE_CONFIG = {
                        device: {
                          label: '设备',
                          color: 'bg-green-500',
                          textColor: 'text-green-600',
                          bgColor: 'bg-green-50 dark:bg-green-950/30',
                          borderColor: 'border-green-200 dark:border-green-800',
                          icon: MapPin,
                          description: '显示设备状态'
                        },
                        metric: {
                          label: '指标',
                          color: 'bg-purple-500',
                          textColor: 'text-purple-600',
                          bgColor: 'bg-purple-50 dark:bg-purple-950/30',
                          borderColor: 'border-purple-200 dark:border-purple-800',
                          icon: Activity,
                          description: '显示指标数值'
                        },
                        command: {
                          label: '指令',
                          color: 'bg-blue-500',
                          textColor: 'text-blue-600',
                          bgColor: 'bg-blue-50 dark:bg-blue-950/30',
                          borderColor: 'border-blue-200 dark:border-blue-800',
                          icon: Zap,
                          description: '快速执行指令'
                        },
                        text: {
                          label: '文本',
                          color: 'bg-gray-500',
                          textColor: 'text-gray-600',
                          bgColor: 'bg-gray-50 dark:bg-gray-950/30',
                          borderColor: 'border-gray-200 dark:border-gray-800',
                          icon: Type,
                          description: '显示静态文本'
                        },
                        icon: {
                          label: '图标',
                          color: 'bg-orange-500',
                          textColor: 'text-orange-600',
                          bgColor: 'bg-orange-50 dark:bg-orange-950/30',
                          borderColor: 'border-orange-200 dark:border-orange-800',
                          icon: Sparkles,
                          description: '显示图标'
                        },
                      } as const

                      if (displayBindings.length === 0) {
                        return (
                          <div className="p-6 text-center text-muted-foreground">
                            <Layers className="h-8 w-8 mx-auto mb-2 opacity-50" />
                            <p className="text-sm">暂无图层项绑定</p>
                            <p className="text-xs mt-1">请先在上方添加数据源，然后点击"打开图层编辑器"设置位置</p>
                          </div>
                        )
                      }

                      return (Object.keys(groupedBindings) as Array<keyof typeof groupedBindings>).map(type => {
                        const typeBindings = groupedBindings[type]
                        if (typeBindings.length === 0) return null

                        const typeConfig = LAYER_TYPE_CONFIG[type]
                        const Icon = typeConfig.icon

                        return (
                          <div key={type} className="border-b last:border-b-0">
                            <div className={`px-3 py-2 ${typeConfig.bgColor} border-b ${typeConfig.borderColor} flex items-center justify-between`}>
                              <div className="flex items-center gap-2">
                                <div className={`w-5 h-5 rounded-full ${typeConfig.color} flex items-center justify-center`}>
                                  <Icon className="h-3 w-3 text-white" />
                                </div>
                                <span className="text-sm font-medium">{typeConfig.label}</span>
                                <span className="text-xs text-muted-foreground">({typeBindings.length})</span>
                              </div>
                              <span className="text-xs text-muted-foreground">{typeConfig.description}</span>
                            </div>

                            <div className="divide-y">
                              {typeBindings.map((binding) => {
                                const positionText = binding.position && binding.position !== 'auto'
                                  ? `(${binding.position.x.toFixed(0)}%, ${binding.position.y.toFixed(0)}%)`
                                  : '中心'

                                const ds = binding.dataSource as any
                                const deviceId = ds?.deviceId
                                const metricId = ds?.metricId || ds?.property
                                const command = ds?.command

                                return (
                                  <div
                                    key={binding.id}
                                    className="flex items-center gap-3 p-3 hover:bg-muted/50 transition-colors"
                                  >
                                    <div className={`w-8 h-8 rounded-full flex items-center justify-center ${typeConfig.color}/20 ${typeConfig.textColor}`}>
                                      <Icon className="h-4 w-4" />
                                    </div>
                                    <div className="flex-1 min-w-0">
                                      <div className="text-sm font-medium truncate">{binding.name}</div>
                                      <div className="flex items-center gap-2 text-xs text-muted-foreground">
                                        <span>{positionText}</span>
                                        {deviceId && <span>• {deviceId.slice(0, 8)}...</span>}
                                        {metricId && <span>• {metricId}</span>}
                                        {command && <span>• {command}</span>}
                                      </div>
                                    </div>
                                  </div>
                                )
                              })}
                            </div>
                          </div>
                        )
                      })
                    })()}
                  </div>
                </div>
              ),
            },
          ],
        }

      // ========== Business Components ==========
      case 'agent-status-card':
      case 'decision-list':
      case 'device-control':
      case 'rule-status-grid':
      case 'transform-list':
        // Business components have minimal config for now
        return {
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="text-center py-8 text-muted-foreground">
                  <p className="text-sm">此组件使用系统数据。</p>
                  <p className="text-xs mt-1">请在设置中配置数据源。</p>
                </div>
              ),
            },
          ],
        }

      default:
        return null
    }
  }

  if (!currentDashboard) {
    // Show loading state only if we're still loading
    if (dashboardsLoading) {
      return (
        <div className="flex items-center justify-center h-screen">
          <div className="text-center">
            <h2 className="text-lg font-medium mb-2">Loading Dashboard...</h2>
          </div>
        </div>
      )
    }

    // No dashboard found - show empty state with create button
    return (
      <div className="flex items-center justify-center h-screen">
        <div className="text-center space-y-4">
          <LayoutDashboard className="h-16 w-16 mx-auto text-muted-foreground" />
          <div>
            <h2 className="text-lg font-medium mb-1">No Dashboard Found</h2>
            <p className="text-sm text-muted-foreground mb-4">
              Create your first dashboard to get started
            </p>
            <Button onClick={() => handleDashboardCreate('Overview')}>
              <Plus className="h-4 w-4 mr-1" />
              Create Dashboard
            </Button>
          </div>
        </div>
      </div>
    )
  }

  return (
    <div className="flex h-full overflow-hidden bg-background">
      {/* Sidebar - Dashboard List */}
      <DashboardListSidebar
        dashboards={dashboards}
        currentDashboardId={currentDashboardId}
        onSwitch={handleDashboardSwitch}
        onCreate={handleDashboardCreate}
        onRename={handleDashboardRename}
        onDelete={handleDashboardDelete}
        open={sidebarOpen}
        onOpenChange={handleSidebarOpenChange}
      />

      {/* Main Content */}
      <div className="flex-1 flex flex-col overflow-hidden">
        {/* Header - fixed at top */}
        <header className="shrink-0 flex items-center justify-between px-4 py-3 border-b border-border bg-background z-10">
          <div className="flex items-center gap-3">
            <Button
              variant="ghost"
              size="icon"
              onClick={() => handleSidebarOpenChange(!sidebarOpen)}
            >
              <PanelsTopLeft className="h-5 w-5" />
            </Button>
            <h1 className="text-lg font-semibold">
              {currentDashboard.name}
            </h1>
          </div>

          <div className="flex items-center gap-2">
            <Button
              variant={editMode ? "default" : "outline"}
              size="sm"
              onClick={() => setEditMode(!editMode)}
              className={editMode ? "shadow-sm" : ""}
            >
              {editMode ? (
                <>
                  <Check className="h-4 w-4 mr-1" />
                  <span className="hidden sm:inline">Done</span>
                  <span className="sm:hidden">Done</span>
                </>
              ) : (
                <>
                  <Settings2 className="h-4 w-4 mr-1" />
                  <span className="hidden sm:inline">Edit Layout</span>
                  <span className="sm:hidden">Edit</span>
                </>
              )}
            </Button>

            <Sheet open={componentLibraryOpen} onOpenChange={(open) => {
              if (editMode) {
                setComponentLibraryOpen(open)
              }
            }}>
              <SheetTrigger asChild>
                <Button
                  variant="default"
                  size="sm"
                  className="shadow-sm"
                  disabled={!editMode}
                >
                  <Plus className="h-4 w-4 mr-1" />
                  <span className="hidden sm:inline">Add</span>
                  <span className="sm:hidden">Add</span>
                </Button>
              </SheetTrigger>
              <SheetContent side="right" className="w-80 sm:w-96 overflow-y-auto">
                <SheetTitle>Component Library</SheetTitle>
                <div className="mt-4 space-y-6 pb-6">
                  {COMPONENT_LIBRARY.map((category) => (
                    <div key={category.category}>
                      <div className="flex items-center gap-2 mb-3">
                        <category.categoryIcon className="h-4 w-4 text-muted-foreground" />
                        <h3 className="text-sm font-medium">{category.categoryLabel}</h3>
                      </div>
                      <div className="grid grid-cols-2 gap-2">
                        {category.items.map((item) => {
                          const Icon = item.icon
                          return (
                            <Button
                              key={item.id}
                              variant="outline"
                              size="sm"
                              className="h-auto flex-col items-start p-3 text-left"
                              onClick={() => handleAddComponent(item.id)}
                            >
                              <Icon className="h-4 w-4 mb-2 text-muted-foreground" />
                              <span className="text-xs font-medium">{item.name}</span>
                              <span className="text-xs text-muted-foreground mt-1">{item.description}</span>
                            </Button>
                          )
                        })}
                      </div>
                    </div>
                  ))}
                </div>
              </SheetContent>
            </Sheet>
          </div>
        </header>

        {/* Dashboard Grid */}
        <div className="flex-1 overflow-auto p-4">
          {currentDashboard.components.length === 0 ? (
            <div className="h-full flex flex-col items-center justify-center text-muted-foreground">
              <LayoutDashboard className="h-16 w-16 mb-4 opacity-50" />
              <p className="text-lg font-medium">Empty Dashboard</p>
              <p className="text-sm mt-2">
                {editMode ? 'Add components to get started' : 'Enter edit mode to add components'}
              </p>
              {editMode && (
                <Button
                  variant="outline"
                  size="sm"
                  className="mt-4"
                  onClick={() => setComponentLibraryOpen(true)}
                >
                  <Plus className="h-4 w-4 mr-1" />
                  Add Component
                </Button>
              )}
            </div>
          ) : (
            <DashboardGrid
              components={gridComponents}
              editMode={editMode}
              onLayoutChange={handleLayoutChange}
            />
          )}
        </div>
      </div>

      {/* Config Dialog */}
      <ComponentConfigDialog
        open={configOpen}
        onClose={handleCancelConfig}
        onSave={handleSaveConfig}
        title={configTitle}
        onTitleChange={handleTitleChange}
        configSchema={configSchema}
        componentType={selectedComponent?.type || ''}
        previewDataSource={componentConfig.dataSource}
        previewConfig={componentConfig}
        showTitleInDisplay={isTitleInDisplayComponent(selectedComponent?.type)}
      />

      {/* Map Editor Dialog */}
      <MapEditorDialog
        open={mapEditorOpen}
        onOpenChange={setMapEditorOpen}
        bindings={mapEditorBindings}
        center={(componentConfig.center as { lat: number; lng: number }) || { lat: 39.9042, lng: 116.4074 }}
        zoom={componentConfig.zoom as number || 10}
        tileLayer={componentConfig.tileLayer as string || 'osm'}
        onSave={handleMapEditorSave}
      />

      {/* Layer Editor Dialog */}
      <LayerEditorDialog
        open={layerEditorOpen}
        onOpenChange={setLayerEditorOpen}
        bindings={layerEditorBindings}
        backgroundType={componentConfig.backgroundType as 'color' | 'image' | 'transparent' | 'grid' || 'grid'}
        backgroundColor={componentConfig.backgroundColor as string}
        backgroundImage={componentConfig.backgroundImage as string}
        onSave={handleLayerEditorSave}
      />
    </div>
  )
}
