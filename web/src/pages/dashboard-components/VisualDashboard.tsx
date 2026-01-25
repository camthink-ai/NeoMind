/**
 * Visual Dashboard Page
 *
 * Main dashboard page with grid layout, drag-and-drop, and component library.
 * Supports both generic IoT components and business components.
 */

import { useEffect, useState, useCallback, useRef, useMemo } from 'react'
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
  RotateCw,
  // Media icons
  Image as ImageIcon,
  Video as VideoIcon,
  Music,
  Globe,
  QrCode,
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
  Box,
  Cloud,
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
  // Tables & Lists
  DataTable,
  LogFeed,
  StatusList,
  // Display & Content
  ImageDisplay,
  ImageHistory,
  WebDisplay,
  MarkdownDisplay,
  // Business
  AgentStatusCard,
  DecisionList,
  DeviceControl,
  RuleStatusGrid,
  TransformList,
} from '@/components/dashboard'
import { DashboardListSidebar } from '@/components/dashboard/DashboardListSidebar'
import type { DashboardComponent, DataSourceOrList, DataSource } from '@/types/dashboard'
import { COMPONENT_SIZE_CONSTRAINTS } from '@/types/dashboard'
import { confirm } from '@/hooks/use-confirm'

// ============================================================================
// Helper Functions
// ============================================================================

/**
 * Memoized cache for converted telemetry data sources
 * Caches both individual DataSource objects AND complete arrays to prevent reference changes
 */
const telemetryCache = new Map<string, DataSourceOrList>()

/**
 * Convert device data source to telemetry with caching to prevent infinite re-renders
 * This function caches the ENTIRE result (including arrays) to ensure reference stability
 */
function getTelemetryDataSource(dataSource: DataSourceOrList | undefined): DataSourceOrList | undefined {
  if (!dataSource) return undefined

  // Create a stable cache key from the entire input
  const cacheKey = JSON.stringify(dataSource)

  // Return cached result if exists (reference stability!)
  if (telemetryCache.has(cacheKey)) {
    console.log('[getTelemetryDataSource] Cache hit, returning cached telemetry')
    return telemetryCache.get(cacheKey)!
  }

  console.log('[getTelemetryDataSource] Cache miss, converting to telemetry:', dataSource)

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
  telemetryCache.set(cacheKey, result)

  return result
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
  // Lists & Tables
  {
    category: 'lists',
    categoryLabel: 'Lists & Tables',
    categoryIcon: List,
    items: [
      { id: 'data-table', name: 'Data Table', description: 'Sortable table', icon: Table },
      { id: 'status-list', name: 'Status List', description: 'Status items list', icon: ListTodo },
      { id: 'log-feed', name: 'Log Feed', description: 'Scrolling log', icon: Scroll },
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

function renderDashboardComponent(component: DashboardComponent) {
  const config = (component as any).config || {}
  const commonProps = getCommonDisplayProps(component)
  const spreadableProps = getSpreadableProps(component.type, commonProps)

  try {
    switch (component.type) {
    // Indicators
    case 'value-card':
      return (
        <ValueCard
          {...spreadableProps}
          dataSource={config.dataSource}
          label={commonProps.title || 'Value'}
          unit={config.unit}
          prefix={config.prefix}
          icon={config.icon}
          iconType={config.iconType || 'entity'}
          description={config.description}
          variant={config.variant || 'default'}
          color={config.color}
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
          dataSource={config.dataSource}
          state={config.state || 'off'}
          label={config.label || commonProps.title}
          size={config.size || 'md'}
          color={config.color}
          valueMap={config.valueMap}
          defaultState={config.defaultState}
          showGlow={config.showGlow ?? true}
          showCard={config.showCard ?? true}
        />
      )

    case 'sparkline':
      return (
        <Sparkline
          {...spreadableProps}
          dataSource={getTelemetryDataSource(config.dataSource)}
          data={config.data}
          showCard={commonProps.showCard}
          showThreshold={config.showThreshold ?? false}
          threshold={config.threshold}
          thresholdColor={config.thresholdColor}
          label={config.label || commonProps.title}
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
          dataSource={config.dataSource}
          value={config.dataSource ? undefined : config.value}
          max={config.max ?? 100}
          label={config.label || commonProps.title}
          color={config.color}
          size={config.size || commonProps.size}
          variant={config.variant || 'default'}
          warningThreshold={config.warningThreshold}
          dangerThreshold={config.dangerThreshold}
          showCard={config.showCard ?? true}
        />
      )

    // Charts
    case 'line-chart':
      return (
        <LineChart
          {...spreadableProps}
          dataSource={config.dataSource}
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
          dataSource={config.dataSource}
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
          dataSource={config.dataSource}
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
          dataSource={config.dataSource}
          data={config.data}
          title={commonProps.title}
          height={getChartHeight(component)}
          limit={config.limit}
          timeRange={config.timeRange}
          showLegend={config.showLegend ?? false}
          showTooltip={config.showTooltip ?? true}
          showLabels={config.showLabels ?? false}
          variant={config.variant || 'pie'}
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
          dataSource={config.dataSource}
          label={config.label || commonProps.title}
          initialState={config.initialState ?? false}
        />
      )

    // Tables & Lists
    case 'data-table':
      return (
        <DataTable
          {...spreadableProps}
          dataSource={config.dataSource}
          columns={config.columns || [
            { key: 'name', label: 'Name' },
            { key: 'value', label: 'Value' }
          ]}
          data={config.data}
          sortable={config.sortable ?? true}
        />
      )

    case 'status-list':
      return (
        <StatusList
          {...spreadableProps}
          dataSource={config.dataSource}
          data={config.data}
          title={commonProps.title}
          showTimestamp={config.showTimestamp ?? true}
          showDescription={config.showDescription ?? true}
        />
      )

    case 'log-feed':
      return (
        <LogFeed
          {...spreadableProps}
          dataSource={config.dataSource}
          data={config.data}
          title={commonProps.title}
          maxEntries={config.maxEntries || 50}
          autoScroll={config.autoScroll ?? true}
          showTimestamp={config.showTimestamp ?? true}
        />
      )

    // Display & Content
    case 'image-display':
      return (
        <ImageDisplay
          {...spreadableProps}
          dataSource={config.dataSource}
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
          dataSource={config.dataSource}
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
          dataSource={config.dataSource}
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
          dataSource={config.dataSource}
          content={config.content}
          variant={config.variant || 'default'}
        />
      )

    // Business Components
    case 'agent-status-card':
      return (
        <AgentStatusCard
          {...spreadableProps}
          dataSource={config.dataSource}
          name={commonProps.title || 'Agent'}
          description={config.description}
          status={config.status}
          executions={config.executions}
          successRate={config.successRate}
          avgDuration={config.avgDuration}
          lastRun={config.lastRun}
        />
      )

    case 'decision-list':
      return (
        <DecisionList
          {...spreadableProps}
          dataSource={config.dataSource}
          decisions={config.decisions}
          title={commonProps.title}
          filter={config.filter}
          showReasoning={config.showReasoning}
          showConfidence={config.showConfidence}
          maxDecisions={config.maxDecisions}
          onApprove={config.onApprove}
          onReject={config.onReject}
          onView={config.onView}
        />
      )

    case 'device-control':
      return (
        <DeviceControl
          {...spreadableProps}
          dataSource={config.dataSource}
          deviceId={config.deviceId}
          commands={config.commands}
          deviceName={config.deviceName}
          deviceStatus={config.deviceStatus}
          title={commonProps.title}
          showStatus={config.showStatus}
          onCommand={config.onCommand}
        />
      )

    case 'rule-status-grid':
      return (
        <RuleStatusGrid
          {...spreadableProps}
          dataSource={config.dataSource}
          rules={config.rules}
          title={commonProps.title}
          showTriggers={config.showTriggers ?? true}
          showErrors={config.showErrors ?? true}
        />
      )

    case 'transform-list':
      return (
        <TransformList
          {...spreadableProps}
          dataSource={config.dataSource}
          transforms={config.transforms}
          title={commonProps.title}
          showSchema={config.showSchema ?? true}
          showStats={config.showStats ?? true}
        />
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

  const {
    currentDashboard,
    currentDashboardId,
    dashboards,
    editMode,
    setEditMode,
    addComponent,
    updateComponent,
    removeComponent,
    duplicateComponent,
    createDashboard,
    updateDashboard,
    deleteDashboard,
    setCurrentDashboard,
    componentLibraryOpen,
    setComponentLibraryOpen,
    fetchDashboards,
    fetchDevices,
    fetchDeviceTypes,
  } = useStore()

  const [configOpen, setConfigOpen] = useState(false)
  const [selectedComponent, setSelectedComponent] = useState<DashboardComponent | null>(null)

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

  // Track if we've initialized to avoid duplicate calls
  const hasInitialized = useRef(false)

  // Track previous components to detect actual changes (not just reference changes)
  const prevComponentsRef = useRef<DashboardComponent[]>([])

  // Create a stable key for components to detect actual changes
  // This key only changes when component data actually changes, not on every render
  const componentsStableKey = useMemo(() => {
    const components = currentDashboard?.components ?? []
    const prevComponents = prevComponentsRef.current ?? []

    // Quick check: if length changed, definitely different
    if (components.length !== prevComponents.length) {
      prevComponentsRef.current = components
      return `changed-${components.length}-${Date.now()}`
    }

    // Deep check: compare each component's key properties
    for (let i = 0; i < components.length; i++) {
      const curr = components[i]
      const prev = prevComponents[i]

      if (!prev) {
        prevComponentsRef.current = components
        return `new-${curr.id}-${curr.type}-${Date.now()}`
      }

      // Check each property separately
      if (curr.id !== prev.id ||
          curr.type !== prev.type ||
          curr.position.x !== prev.position.x ||
          curr.position.y !== prev.position.y ||
          curr.position.w !== prev.position.w ||
          curr.position.h !== prev.position.h ||
          JSON.stringify(curr.config) !== JSON.stringify(prev.config)) {
        prevComponentsRef.current = components
        return `changed-${curr.id}-${Date.now()}`
      }
    }

    // No actual changes detected - return previous key
    return `stable-${components.length}`
  }, [currentDashboard?.components])

  // Use a counter to force refresh when config changes in dialog
  // Must be declared before gridComponents which depends on it
  const [configVersion, setConfigVersion] = useState(0)

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

  // Re-load dashboards if array becomes empty but we have a current ID
  useEffect(() => {
    if (dashboards.length === 0 && currentDashboardId) {
      // Try to recover by fetching again
      fetchDashboards()
    }
  }, [dashboards.length, currentDashboardId, fetchDashboards])

  // Create default dashboard if needed
  useEffect(() => {
    if (dashboards.length === 0 && !currentDashboard) {
      createDashboard({
        name: 'Overview',
        layout: {
          columns: 12,
          rows: 'auto' as const,
          breakpoints: { lg: 1200, md: 996, sm: 768, xs: 480 },
        },
        components: [],
      })
    }
  }, [dashboards.length, currentDashboard, createDashboard])

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
      // Tables & Lists
      case 'data-table':
        defaultConfig = {
          columns: [{ key: 'name', label: 'Name' }, { key: 'value', label: 'Value' }],
          data: [{ name: 'Item 1', value: 100 }, { name: 'Item 2', value: 200 }]
        }
        break
      case 'status-list':
        defaultConfig = {
          data: [
            { id: '1', label: 'Online', status: 'online' },
            { id: '2', label: 'Offline', status: 'offline' }
          ]
        }
        break
      case 'log-feed':
        defaultConfig = {
          data: [
            { id: '1', message: 'System started', level: 'info', timestamp: new Date().toISOString() }
          ]
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
      // Business Components
      case 'agent-status-card':
        defaultConfig = {
          name: 'Agent',
          status: 'online',
          executions: 0
        }
        break
      case 'device-control':
        defaultConfig = {
          commands: [
            { id: 'cmd1', name: 'Toggle', type: 'toggle' }
          ]
        }
        break
      case 'rule-status-grid':
        defaultConfig = {
          rules: []
        }
        break
      case 'transform-list':
        defaultConfig = {
          transforms: []
        }
        break
      case 'decision-list':
        defaultConfig = {
          decisions: []
        }
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
    const config = { ...((component as any).config || {}) }
    setConfigTitle(component.title || 'Configure Component')
    setComponentConfig(config)
    setConfigOpen(true)
  }, [currentDashboard?.components])

  // Memoize grid components to prevent infinite re-renders
  // Only recalculate when actual component data changes (detected via stableKey)
  // Note: handleOpenConfig, removeComponent, duplicateComponent are NOT dependencies
  // because they don't affect the rendered output structure, only event handlers
  const gridComponents = useMemo(() => {
    return currentDashboard?.components.map((component) => ({
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
          {renderDashboardComponent(component)}
        </ComponentWrapper>
      ),
    })) ?? []
  }, [componentsStableKey, editMode, configVersion])

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
        // Update the component with current config for live preview
        updateComponent(selectedComponent.id, { config: componentConfig })
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

  // Handle saving component config (just close dialog, already live-previewed)
  const handleSaveConfig = () => {
    setConfigOpen(false)
  }

  // Handle title change
  const handleTitleChange = (newTitle: string) => {
    setConfigTitle(newTitle)
    if (selectedComponent) {
      updateComponent(selectedComponent.id, { title: newTitle })
    }
  }

  // Generate config schema based on component type
  const generateConfigSchema = (componentType: string, currentConfig: any): ComponentConfigSchema | null => {
    const config = currentConfig || {}

    // Helper to create updater functions
    const updateConfig = (key: string) => (value: any) => {
      setComponentConfig(prev => ({ ...prev, [key]: value }))
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
                <div className="space-y-4">
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Variant</label>
                    <select
                      value={config.variant || 'default'}
                      onChange={(e) => updateConfig('variant')(e.target.value)}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    >
                      <option value="default">Default (horizontal)</option>
                      <option value="vertical">Vertical</option>
                      <option value="compact">Compact</option>
                      <option value="minimal">Minimal</option>
                    </select>
                  </div>

                  <div className="space-y-2">
                    <label className="text-sm font-medium">Icon</label>
                    <select
                      value={config.icon || ''}
                      onChange={(e) => updateConfig('icon')(e.target.value)}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    >
                      <option value="">No Icon</option>
                      <option value="activity">Activity</option>
                      <option value="cpu">CPU</option>
                      <option value="memory">Memory</option>
                      <option value="temperature">Temperature</option>
                      <option value="humidity">Humidity</option>
                      <option value="speed">Speed</option>
                      <option value="power">Power</option>
                      <option value="battery">Battery</option>
                      <option value="wifi">WiFi</option>
                      <option value="database">Database</option>
                    </select>
                  </div>

                  {config.icon && (
                    <div className="space-y-2">
                      <label className="text-sm font-medium">Icon Type</label>
                      <select
                        value={config.iconType || 'entity'}
                        onChange={(e) => updateConfig('iconType')(e.target.value)}
                        className="w-full h-10 px-3 rounded-md border border-input bg-background"
                      >
                        <option value="entity">Entity Icon</option>
                        <option value="emoji">Emoji</option>
                      </select>
                    </div>
                  )}

                  <div className="space-y-2">
                    <label className="text-sm font-medium">Color</label>
                    <input
                      type="color"
                      value={config.color || '#3b82f6'}
                      onChange={(e) => updateConfig('color')(e.target.value)}
                      className="w-full h-10 rounded-md border border-input bg-background"
                    />
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
                  <div className="grid grid-cols-2 gap-2">
                    <div className="space-y-2">
                      <label className="text-sm font-medium">Prefix</label>
                      <input
                        type="text"
                        value={config.prefix || ''}
                        onChange={(e) => updateConfig('prefix')(e.target.value)}
                        placeholder="e.g., $, °"
                        className="w-full h-10 px-3 rounded-md border border-input bg-background"
                      />
                    </div>

                    <div className="space-y-2">
                      <label className="text-sm font-medium">Unit</label>
                      <input
                        type="text"
                        value={config.unit || ''}
                        onChange={(e) => updateConfig('unit')(e.target.value)}
                        placeholder="e.g., %, °C"
                        className="w-full h-10 px-3 rounded-md border border-input bg-background"
                      />
                    </div>
                  </div>

                  <div className="space-y-2">
                    <label className="text-sm font-medium">Description</label>
                    <input
                      type="text"
                      value={config.description || ''}
                      onChange={(e) => updateConfig('description')(e.target.value)}
                      placeholder="e.g., Current CPU usage"
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    />
                  </div>

                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showTrend ?? false}
                        onChange={(e) => updateConfig('showTrend')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Show Trend</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showSparkline ?? false}
                        onChange={(e) => updateConfig('showSparkline')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Show Sparkline</span>
                    </label>
                  </div>

                  {config.showTrend && (
                    <div className="space-y-2">
                      <label className="text-sm font-medium">Trend Value (%)</label>
                      <input
                        type="number"
                        value={config.trendValue ?? 0}
                        onChange={(e) => updateConfig('trendValue')(Number(e.target.value))}
                        className="w-full h-10 px-3 rounded-md border border-input bg-background"
                      />
                    </div>
                  )}
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
                <div className="space-y-4">
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Color Mode</label>
                    <select
                      value={config.colorMode || 'auto'}
                      onChange={(e) => updateConfig('colorMode')(e.target.value)}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    >
                      <option value="auto">Auto (trend-based)</option>
                      <option value="primary">Primary</option>
                      <option value="fixed">Fixed Color</option>
                      <option value="value">Value-based</option>
                    </select>
                  </div>

                  <div className="space-y-2">
                    <label className="text-sm font-medium">Color (for fixed mode)</label>
                    <input
                      type="color"
                      value={config.color || '#3b82f6'}
                      onChange={(e) => updateConfig('color')(e.target.value)}
                      className="w-full h-10 rounded-md border border-input bg-background"
                    />
                  </div>

                  <div className="space-y-2">
                    <label className="text-sm font-medium">Max Value (for value-based coloring)</label>
                    <input
                      type="number"
                      value={config.maxValue || 100}
                      onChange={(e) => updateConfig('maxValue')(Number(e.target.value))}
                      min={1}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    />
                  </div>

                  <div className="space-y-2">
                    <label className="text-sm font-medium">Stroke Width</label>
                    <input
                      type="number"
                      value={config.strokeWidth ?? 2}
                      onChange={(e) => updateConfig('strokeWidth')(Number(e.target.value))}
                      min={1}
                      max={5}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    />
                  </div>

                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.fill ?? true}
                        onChange={(e) => updateConfig('fill')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Fill Area</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.curved ?? true}
                        onChange={(e) => updateConfig('curved')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Curved Lines</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showPoints ?? false}
                        onChange={(e) => updateConfig('showPoints')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Show Points</span>
                    </label>
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
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Label</label>
                    <input
                      type="text"
                      value={config.label || ''}
                      onChange={(e) => updateConfig('label')(e.target.value)}
                      placeholder="e.g., Temperature Trend"
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    />
                  </div>

                  <label className="flex items-center gap-2 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={config.showValue ?? false}
                      onChange={(e) => updateConfig('showValue')(e.target.checked)}
                      className="rounded"
                    />
                    <span className="text-sm">Show Current Value</span>
                  </label>

                  <label className="flex items-center gap-2 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={config.showThreshold ?? false}
                      onChange={(e) => updateConfig('showThreshold')(e.target.checked)}
                      className="rounded"
                    />
                    <span className="text-sm">Show Threshold Line</span>
                  </label>

                  {config.showThreshold && (
                    <>
                      <div className="space-y-2">
                        <label className="text-sm font-medium">Threshold Value</label>
                        <input
                          type="number"
                          value={config.threshold ?? 20}
                          onChange={(e) => updateConfig('threshold')(Number(e.target.value))}
                          className="w-full h-10 px-3 rounded-md border border-input bg-background"
                        />
                      </div>

                      <div className="space-y-2">
                        <label className="text-sm font-medium">Threshold Color</label>
                        <input
                          type="color"
                          value={config.thresholdColor || '#ef4444'}
                          onChange={(e) => updateConfig('thresholdColor')(e.target.value)}
                          className="w-full h-10 rounded-md border border-input bg-background"
                        />
                      </div>
                    </>
                  )}
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
                <div className="space-y-4">
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Variant</label>
                    <select
                      value={config.variant || 'default'}
                      onChange={(e) => updateConfig('variant')(e.target.value)}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    >
                      <option value="default">Default (Linear)</option>
                      <option value="compact">Compact</option>
                      <option value="circular">Circular</option>
                    </select>
                  </div>

                  <div className="space-y-2">
                    <label className="text-sm font-medium">Color</label>
                    <input
                      type="color"
                      value={config.color || '#3b82f6'}
                      onChange={(e) => updateConfig('color')(e.target.value)}
                      className="w-full h-10 rounded-md border border-input bg-background"
                    />
                  </div>

                  <div className="space-y-2">
                    <label className="text-sm font-medium">Size</label>
                    <select
                      value={config.size || 'md'}
                      onChange={(e) => updateConfig('size')(e.target.value)}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    >
                      <option value="sm">Small</option>
                      <option value="md">Medium</option>
                      <option value="lg">Large</option>
                    </select>
                  </div>

                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showCard ?? true}
                        onChange={(e) => updateConfig('showCard')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Show Card</span>
                    </label>
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
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Label</label>
                    <input
                      type="text"
                      value={config.label || ''}
                      onChange={(e) => updateConfig('label')(e.target.value)}
                      placeholder="e.g., CPU Usage"
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    />
                  </div>

                  <div className="grid grid-cols-2 gap-3">
                    <div className="space-y-2">
                      <label className="text-sm font-medium">Warning Threshold (%)</label>
                      <input
                        type="number"
                        value={config.warningThreshold ?? 70}
                        onChange={(e) => updateConfig('warningThreshold')(Number(e.target.value))}
                        min={0}
                        max={100}
                        className="w-full h-10 px-3 rounded-md border border-input bg-background"
                      />
                    </div>
                    <div className="space-y-2">
                      <label className="text-sm font-medium">Danger Threshold (%)</label>
                      <input
                        type="number"
                        value={config.dangerThreshold ?? 90}
                        onChange={(e) => updateConfig('dangerThreshold')(Number(e.target.value))}
                        min={0}
                        max={100}
                        className="w-full h-10 px-3 rounded-md border border-input bg-background"
                      />
                    </div>
                  </div>
                  <p className="text-xs text-muted-foreground">
                    进度条颜色会根据阈值自动变化：正常 → 警告 → 危险
                  </p>
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
              },
            },
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-4">
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Value (静态)</label>
                    <input
                      type="number"
                      value={config.value ?? 0}
                      onChange={(e) => updateConfig('value')(Number(e.target.value))}
                      min={0}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                      disabled={!!config.dataSource}
                    />
                    <p className="text-xs text-muted-foreground">绑定数据源后自动禁用</p>
                  </div>

                  <div className="space-y-2">
                    <label className="text-sm font-medium">Maximum Value</label>
                    <input
                      type="number"
                      value={config.max ?? 100}
                      onChange={(e) => updateConfig('max')(Number(e.target.value))}
                      min={1}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    />
                  </div>
                </div>
              ),
            },
          ],
        }

      case 'led-indicator':
        return {
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-4">
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Label</label>
                    <input
                      type="text"
                      value={config.label || ''}
                      onChange={(e) => updateConfig('label')(e.target.value)}
                      placeholder="e.g., Device Status"
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    />
                  </div>

                  <div className="space-y-2">
                    <label className="text-sm font-medium">State (no data source)</label>
                    <select
                      value={config.state || 'on'}
                      onChange={(e) => updateConfig('state')(e.target.value)}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    >
                      <option value="on">On</option>
                      <option value="off">Off</option>
                      <option value="error">Error</option>
                      <option value="warning">Warning</option>
                    </select>
                  </div>

                  <div className="space-y-2">
                    <label className="text-sm font-medium">Size</label>
                    <select
                      value={config.size || 'md'}
                      onChange={(e) => updateConfig('size')(e.target.value)}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    >
                      <option value="sm">Small</option>
                      <option value="md">Medium</option>
                      <option value="lg">Large</option>
                    </select>
                  </div>

                  <div className="space-y-2">
                    <label className="text-sm font-medium">Color</label>
                    <input
                      type="color"
                      value={config.color || '#22c55e'}
                      onChange={(e) => updateConfig('color')(e.target.value)}
                      className="w-full h-10 rounded-md border border-input bg-background"
                    />
                  </div>

                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showGlow ?? true}
                        onChange={(e) => updateConfig('showGlow')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Show Glow Effect</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showCard ?? true}
                        onChange={(e) => updateConfig('showCard')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Show Card</span>
                    </label>
                  </div>
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
              },
            },
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-4">
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Value Mapping</label>
                    <p className="text-xs text-muted-foreground">
                      One mapping per line: <code>values {'->'} state</code>
                    </p>
                    <textarea
                      value={(config.valueMap || [])
                        .map((m: any) => {
                          const values = m.values || m.pattern || ''
                          const label = m.label ? ` (${m.label})` : ''
                          const color = m.color ? ` [${m.color}]` : ''
                          return `${values} -> ${m.state}${label}${color}`
                        })
                        .join('\n')}
                      onChange={(e) => {
                        const lines = e.target.value.split('\n').filter(Boolean)
                        const newValueMap = lines.map(line => {
                          const match = line.match(/^(.+?)\s*->\s*(\w+)(?:\s*\[([#\w]+)\])?(?:\s*\((.+?)\))?$/)
                          if (match) {
                            const [, valuesOrPattern, state, color, label] = match
                            const trimmed = valuesOrPattern.trim()
                            const isPattern = /[.*+?^${}()|[\]\\]/.test(trimmed) && !trimmed.includes(',')
                            return {
                              state,
                              ...(isPattern ? { pattern: trimmed } : { values: trimmed }),
                              ...(color && { color }),
                              ...(label && { label }),
                            }
                          }
                          return { state: 'unknown', values: line }
                        })
                        updateConfig('valueMap')(newValueMap)
                      }}
                      placeholder="online, active -> on\noffline -> off\nerror, failed -> error [ef4444]\n/warn|warning/i -> warning"
                      className="w-full h-32 px-3 py-2 rounded-md border border-input bg-background text-sm font-mono"
                    />
                  </div>

                  <div className="space-y-2">
                    <label className="text-sm font-medium">Default State</label>
                    <select
                      value={config.defaultState || 'unknown'}
                      onChange={(e) => updateConfig('defaultState')(e.target.value)}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    >
                      <option value="on">On</option>
                      <option value="off">Off</option>
                      <option value="error">Error</option>
                      <option value="warning">Warning</option>
                      <option value="unknown">Unknown</option>
                    </select>
                  </div>
                </div>
              ),
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
                <div className="space-y-4">
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Line Color</label>
                    <input
                      type="color"
                      value={config.color || '#3b82f6'}
                      onChange={(e) => updateConfig('color')(e.target.value)}
                      className="w-full h-10 rounded-md border border-input bg-background"
                    />
                  </div>

                  <div className="space-y-2">
                    <label className="text-sm font-medium">Size</label>
                    <select
                      value={config.size || 'md'}
                      onChange={(e) => updateConfig('size')(e.target.value)}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    >
                      <option value="sm">Small</option>
                      <option value="md">Medium</option>
                      <option value="lg">Large</option>
                    </select>
                  </div>

                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.smooth ?? true}
                        onChange={(e) => updateConfig('smooth')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Smooth Lines</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.fillArea ?? false}
                        onChange={(e) => updateConfig('fillArea')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Fill Area</span>
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
                      <span className="text-sm">Show Grid</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showLegend ?? false}
                        onChange={(e) => updateConfig('showLegend')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Show Legend</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showTooltip ?? true}
                        onChange={(e) => updateConfig('showTooltip')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Show Tooltip</span>
                    </label>
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
                  <div className="grid grid-cols-2 gap-3">
                    <div className="space-y-2">
                      <label className="text-sm font-medium">Data Points</label>
                      <input
                        type="number"
                        value={config.limit ?? 50}
                        onChange={(e) => updateConfig('limit')(Number(e.target.value))}
                        min={1}
                        max={200}
                        className="w-full h-10 px-3 rounded-md border border-input bg-background text-sm"
                      />
                      <p className="text-xs text-muted-foreground">Max points to fetch</p>
                    </div>
                    <div className="space-y-2">
                      <label className="text-sm font-medium">Time Range (hours)</label>
                      <input
                        type="number"
                        value={config.timeRange ?? 1}
                        onChange={(e) => updateConfig('timeRange')(Number(e.target.value))}
                        min={1}
                        max={168}
                        step={1}
                        className="w-full h-10 px-3 rounded-md border border-input bg-background text-sm"
                      />
                      <p className="text-xs text-muted-foreground">Historical period</p>
                    </div>
                  </div>
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
                <div className="space-y-4">
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Area Color</label>
                    <input
                      type="color"
                      value={config.color || '#3b82f6'}
                      onChange={(e) => updateConfig('color')(e.target.value)}
                      className="w-full h-10 rounded-md border border-input bg-background"
                    />
                  </div>

                  <div className="space-y-2">
                    <label className="text-sm font-medium">Size</label>
                    <select
                      value={config.size || 'md'}
                      onChange={(e) => updateConfig('size')(e.target.value)}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    >
                      <option value="sm">Small</option>
                      <option value="md">Medium</option>
                      <option value="lg">Large</option>
                    </select>
                  </div>

                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.smooth ?? true}
                        onChange={(e) => updateConfig('smooth')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Smooth Lines</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showGrid ?? true}
                        onChange={(e) => updateConfig('showGrid')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Show Grid</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showLegend ?? false}
                        onChange={(e) => updateConfig('showLegend')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Show Legend</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showTooltip ?? true}
                        onChange={(e) => updateConfig('showTooltip')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Show Tooltip</span>
                    </label>
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
                  <div className="grid grid-cols-2 gap-3">
                    <div className="space-y-2">
                      <label className="text-sm font-medium">Data Points</label>
                      <input
                        type="number"
                        value={config.limit ?? 50}
                        onChange={(e) => updateConfig('limit')(Number(e.target.value))}
                        min={1}
                        max={200}
                        className="w-full h-10 px-3 rounded-md border border-input bg-background text-sm"
                      />
                      <p className="text-xs text-muted-foreground">Max points to fetch</p>
                    </div>
                    <div className="space-y-2">
                      <label className="text-sm font-medium">Time Range (hours)</label>
                      <input
                        type="number"
                        value={config.timeRange ?? 1}
                        onChange={(e) => updateConfig('timeRange')(Number(e.target.value))}
                        min={1}
                        max={168}
                        step={1}
                        className="w-full h-10 px-3 rounded-md border border-input bg-background text-sm"
                      />
                      <p className="text-xs text-muted-foreground">Historical period</p>
                    </div>
                  </div>
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
                allowedTypes: ['device-metric'],
                multiple: true,
                maxSources: 5,
              },
            },
          ],
        }

      case 'bar-chart':
        return {
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-4">
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Layout</label>
                    <select
                      value={config.layout || 'vertical'}
                      onChange={(e) => updateConfig('layout')(e.target.value)}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    >
                      <option value="vertical">Vertical</option>
                      <option value="horizontal">Horizontal</option>
                    </select>
                  </div>

                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.stacked ?? false}
                        onChange={(e) => updateConfig('stacked')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Stacked</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showGrid ?? true}
                        onChange={(e) => updateConfig('showGrid')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Show Grid</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showLegend ?? false}
                        onChange={(e) => updateConfig('showLegend')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Show Legend</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showTooltip ?? true}
                        onChange={(e) => updateConfig('showTooltip')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Show Tooltip</span>
                    </label>
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
                  <div className="grid grid-cols-2 gap-3">
                    <div className="space-y-2">
                      <label className="text-sm font-medium">Data Points</label>
                      <input
                        type="number"
                        value={config.limit ?? 24}
                        onChange={(e) => updateConfig('limit')(Number(e.target.value))}
                        min={1}
                        max={200}
                        className="w-full h-10 px-3 rounded-md border border-input bg-background text-sm"
                      />
                      <p className="text-xs text-muted-foreground">Max points to fetch</p>
                    </div>
                    <div className="space-y-2">
                      <label className="text-sm font-medium">Time Range (hours)</label>
                      <input
                        type="number"
                        value={config.timeRange ?? 1}
                        onChange={(e) => updateConfig('timeRange')(Number(e.target.value))}
                        min={1}
                        max={168}
                        step={1}
                        className="w-full h-10 px-3 rounded-md border border-input bg-background text-sm"
                      />
                      <p className="text-xs text-muted-foreground">Historical period</p>
                    </div>
                  </div>
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
                <div className="space-y-4">
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Variant</label>
                    <select
                      value={config.variant || 'pie'}
                      onChange={(e) => updateConfig('variant')(e.target.value)}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    >
                      <option value="pie">Pie Chart</option>
                      <option value="donut">Donut Chart</option>
                    </select>
                  </div>

                  {config.variant === 'donut' && (
                    <div className="space-y-2">
                      <label className="text-sm font-medium">Inner Radius</label>
                      <input
                        type="text"
                        value={config.innerRadius || '60%'}
                        onChange={(e) => updateConfig('innerRadius')(e.target.value)}
                        placeholder="60% or 60"
                        className="w-full h-10 px-3 rounded-md border border-input bg-background"
                      />
                    </div>
                  )}

                  <div className="space-y-2">
                    <label className="text-sm font-medium">Outer Radius</label>
                    <input
                      type="text"
                      value={config.outerRadius || '80%'}
                      onChange={(e) => updateConfig('outerRadius')(e.target.value)}
                      placeholder="80% or 80"
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    />
                  </div>

                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showLegend ?? false}
                        onChange={(e) => updateConfig('showLegend')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Show Legend</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showTooltip ?? true}
                        onChange={(e) => updateConfig('showTooltip')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Show Tooltip</span>
                    </label>

                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={config.showLabels ?? false}
                        onChange={(e) => updateConfig('showLabels')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Show Labels</span>
                    </label>
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
                  <div className="grid grid-cols-2 gap-3">
                    <div className="space-y-2">
                      <label className="text-sm font-medium">Data Points</label>
                      <input
                        type="number"
                        value={config.limit ?? 10}
                        onChange={(e) => updateConfig('limit')(Number(e.target.value))}
                        min={1}
                        max={100}
                        className="w-full h-10 px-3 rounded-md border border-input bg-background text-sm"
                      />
                      <p className="text-xs text-muted-foreground">Max categories to show</p>
                    </div>
                    <div className="space-y-2">
                      <label className="text-sm font-medium">Time Range (hours)</label>
                      <input
                        type="number"
                        value={config.timeRange ?? 1}
                        onChange={(e) => updateConfig('timeRange')(Number(e.target.value))}
                        min={1}
                        max={168}
                        step={1}
                        className="w-full h-10 px-3 rounded-md border border-input bg-background text-sm"
                      />
                      <p className="text-xs text-muted-foreground">Historical period</p>
                    </div>
                  </div>
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
                <div className="space-y-4">
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Label</label>
                    <input
                      type="text"
                      value={config.label || ''}
                      onChange={(e) => updateConfig('label')(e.target.value)}
                      placeholder="e.g., Main Light"
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    />
                  </div>

                  <div className="space-y-2">
                    <label className="text-sm font-medium">Initial State</label>
                    <select
                      value={config.initialState ? 'on' : 'off'}
                      onChange={(e) => updateConfig('initialState')(e.target.value === 'on')}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    >
                      <option value="off">Off (关闭)</option>
                      <option value="on">On (开启)</option>
                    </select>
                    <p className="text-xs text-muted-foreground">显示状态，在收到命令响应前使用</p>
                  </div>

                  <div className="space-y-2">
                    <label className="text-sm font-medium">Size</label>
                    <select
                      value={config.size || 'md'}
                      onChange={(e) => updateConfig('size')(e.target.value)}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    >
                      <option value="sm">Small</option>
                      <option value="md">Medium</option>
                      <option value="lg">Large</option>
                    </select>
                  </div>
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
                requireCommand: true,
              },
            },
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-4">
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

      // ========== Tables & Lists ==========
      case 'data-table':
        return {
          dataSourceSections: [
            {
              type: 'data-source' as const,
              props: {
                dataSource: config.dataSource,
                onChange: updateDataSource,
              },
            },
          ],
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-4">
                  <div className="flex items-center gap-2">
                    <input
                      type="checkbox"
                      id="sortable"
                      checked={config.sortable ?? true}
                      onChange={(e) => updateConfig('sortable')(e.target.checked)}
                      className="rounded"
                    />
                    <label htmlFor="sortable" className="text-sm">Sortable</label>
                  </div>
                </div>
              ),
            },
          ],
        }

      case 'status-list':
      case 'log-feed':
        return {
          dataSourceSections: [
            {
              type: 'data-source' as const,
              props: {
                dataSource: config.dataSource,
                onChange: updateDataSource,
              },
            },
          ],
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-4">
                  <div className="flex items-center gap-2">
                    <input
                      type="checkbox"
                      id="showTimestamp"
                      checked={config.showTimestamp ?? true}
                      onChange={(e) => updateConfig('showTimestamp')(e.target.checked)}
                      className="rounded"
                    />
                    <label htmlFor="showTimestamp" className="text-sm">Show Timestamp</label>
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
              },
            },
          ],
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-4">
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Image Source</label>
                    <input
                      type="text"
                      value={config.src || ''}
                      onChange={(e) => updateConfig('src')(e.target.value)}
                      placeholder="https://example.com/image.jpg or data:image/png;base64,..."
                      className="w-full h-10 px-3 rounded-md border border-input bg-background text-sm"
                    />
                    <p className="text-xs text-muted-foreground mt-1">
                      Supports URLs or Base64 (data:image/png;base64,...)
                    </p>
                  </div>
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Fit Mode</label>
                    <select
                      value={config.fit || 'contain'}
                      onChange={(e) => updateConfig('fit')(e.target.value)}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background text-sm"
                    >
                      <option value="contain">Contain</option>
                      <option value="cover">Cover</option>
                      <option value="fill">Fill</option>
                      <option value="none">None</option>
                      <option value="scale-down">Scale Down</option>
                    </select>
                  </div>
                  <div className="flex items-center gap-4">
                    <label className="flex items-center gap-2">
                      <input
                        type="checkbox"
                        checked={config.rounded ?? true}
                        onChange={(e) => updateConfig('rounded')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-xs">Rounded</span>
                    </label>
                    <label className="flex items-center gap-2">
                      <input
                        type="checkbox"
                        checked={config.zoomable ?? true}
                        onChange={(e) => updateConfig('zoomable')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-xs">Zoomable</span>
                    </label>
                    <label className="flex items-center gap-2">
                      <input
                        type="checkbox"
                        checked={config.showShadow ?? false}
                        onChange={(e) => updateConfig('showShadow')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-xs">Shadow</span>
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
              },
            },
          ],
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-4">
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Fit Mode</label>
                    <select
                      value={config.fit || 'contain'}
                      onChange={(e) => updateConfig('fit')(e.target.value)}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background text-sm"
                    >
                      <option value="contain">Contain</option>
                      <option value="cover">Cover</option>
                      <option value="fill">Fill</option>
                      <option value="none">None</option>
                      <option value="scale-down">Scale Down</option>
                    </select>
                  </div>
                  <div className="grid grid-cols-2 gap-4">
                    <div className="space-y-2">
                      <label className="text-sm font-medium">Max Images</label>
                      <input
                        type="number"
                        value={config.limit ?? 50}
                        onChange={(e) => updateConfig('limit')(Number(e.target.value))}
                        min={1}
                        max={200}
                        className="w-full h-10 px-3 rounded-md border border-input bg-background text-sm"
                      />
                    </div>
                    <div className="space-y-2">
                      <label className="text-sm font-medium">Time Range (hours)</label>
                      <input
                        type="number"
                        value={config.timeRange ?? 1}
                        onChange={(e) => updateConfig('timeRange')(Number(e.target.value))}
                        min={1}
                        max={168}
                        step={1}
                        className="w-full h-10 px-3 rounded-md border border-input bg-background text-sm"
                      />
                    </div>
                  </div>
                  <div className="flex flex-wrap items-center gap-3">
                    <label className="flex items-center gap-2">
                      <input
                        type="checkbox"
                        checked={config.rounded ?? true}
                        onChange={(e) => updateConfig('rounded')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-xs">Rounded</span>
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
              },
            },
          ],
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-4">
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Website URL</label>
                    <input
                      type="text"
                      value={config.src || ''}
                      onChange={(e) => updateConfig('src')(e.target.value)}
                      placeholder="https://example.com"
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
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
                      <span className="text-sm">Sandboxed</span>
                    </label>
                    <label className="flex items-center gap-2">
                      <input
                        type="checkbox"
                        checked={config.showHeader ?? true}
                        onChange={(e) => updateConfig('showHeader')(e.target.checked)}
                        className="rounded"
                      />
                      <span className="text-sm">Show Header</span>
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
              },
            },
          ],
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-4">
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Markdown Content</label>
                    <textarea
                      value={config.content || ''}
                      onChange={(e) => updateConfig('content')(e.target.value)}
                      placeholder="# Title\n\n**Bold** and *italic* text"
                      rows={6}
                      className="w-full px-3 py-2 rounded-md border border-input bg-background text-sm"
                    />
                  </div>
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Variant</label>
                    <select
                      value={config.variant || 'default'}
                      onChange={(e) => updateConfig('variant')(e.target.value)}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    >
                      <option value="default">Default</option>
                      <option value="compact">Compact</option>
                      <option value="minimal">Minimal</option>
                    </select>
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
                  <p className="text-sm">This component uses data from the system.</p>
                  <p className="text-xs mt-1">Configure data sources in the settings.</p>
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
    return (
      <div className="flex items-center justify-center h-screen">
        <div className="text-center">
          <h2 className="text-lg font-medium mb-2">Loading Dashboard...</h2>
        </div>
      </div>
    )
  }

  return (
    <div className="flex h-screen overflow-hidden bg-background">
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
        {/* Header */}
        <header className="flex items-center justify-between px-4 py-3 border-b border-border bg-background">
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
        onClose={() => setConfigOpen(false)}
        onSave={handleSaveConfig}
        title={configTitle}
        onTitleChange={handleTitleChange}
        configSchema={configSchema}
        componentType={selectedComponent?.type || ''}
        previewDataSource={componentConfig.dataSource}
        previewConfig={componentConfig}
      />
    </div>
  )
}
