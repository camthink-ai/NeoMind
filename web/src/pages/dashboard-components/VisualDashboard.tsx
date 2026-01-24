/**
 * Visual Dashboard Page
 *
 * Main dashboard page with grid layout, drag-and-drop, and component library.
 * Supports both generic IoT components and business components.
 */

import { useEffect, useState, useCallback, useRef, useMemo } from 'react'
import { useStore } from '@/store'
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
  Gauge as GaugeIcon,
  // Chart icons
  LineChart as LineChartIcon,
  BarChart3,
  Gauge,
  PieChart as PieChartIcon,
  ScatterChart as ScatterChartIcon,
  Donut as DonutIcon,
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

// Dashboard components - Only essential 27 components
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
  DonutChart,
  GaugeChart,
  // Controls
  ToggleSwitch,
  ButtonGroup,
  Dropdown,
  InputField,
  // Tables & Lists
  DataTable,
  LogFeed,
  StatusList,
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
      { id: 'donut-chart', name: 'Donut Chart', description: 'Hollow pie chart', icon: DonutIcon },
      { id: 'gauge-chart', name: 'Gauge Chart', description: 'Value in range', icon: Gauge },
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
  // Controls
  {
    category: 'controls',
    categoryLabel: 'Controls',
    categoryIcon: SlidersHorizontal,
    items: [
      { id: 'toggle-switch', name: 'Toggle Switch', description: 'On/off toggle', icon: ToggleLeft },
      { id: 'button-group', name: 'Button Group', description: 'Action buttons', icon: Layers },
      { id: 'dropdown', name: 'Dropdown', description: 'Select dropdown', icon: List },
      { id: 'input-field', name: 'Input Field', description: 'Text input', icon: Type },
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
  return {
    size: (config.size as 'sm' | 'md' | 'lg') || 'md',
    showCard: config.showCard ?? true,
    className: config.className,
    title: component.title,
    color: config.color,
  }
}

// Props that can be safely spread to most components
const getSpreadableProps = (componentType: string, commonProps: ReturnType<typeof getCommonDisplayProps>) => {
  // Components that don't support standard size ('sm' | 'md' | 'lg')
  const noStandardSize = [
    'gauge-chart', 'led-indicator',
    'toggle-switch', 'button-group', 'dropdown', 'input-field',
    'heading', 'alert-banner',
    'agent-status-card', 'device-control', 'rule-status-grid', 'transform-list',
  ]

  // Components that don't support showCard
  const noShowCard = [
    'value-card', 'led-indicator', 'sparkline', 'progress-bar',
    'toggle-switch', 'button-group', 'dropdown', 'input-field',
    'heading', 'alert-banner',
    'agent-status-card', 'device-control', 'rule-status-grid', 'transform-list',
    'tabs',
  ]

  // Components that don't support title in the spread position
  const noTitle = [
    'sparkline', 'led-indicator', 'progress-bar',
    'toggle-switch', 'button-group', 'dropdown', 'input-field',
    'heading', 'alert-banner',
    'tabs',
    'agent-status-card', 'device-control', 'rule-status-grid', 'transform-list',
  ]

  const result: Record<string, unknown> = {}

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
          showTrend={config.showTrend}
          trendValue={config.trendValue}
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
          size={config.ledSize || 'md'}
          color={config.color}
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
          threshold={config.threshold ?? 20}
          label={config.label || commonProps.title}
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
          height="auto"
          title={commonProps.title}
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
          height="auto"
          title={commonProps.title}
        />
      )

    case 'bar-chart':
      return (
        <BarChart
          {...spreadableProps}
          dataSource={config.dataSource}
          data={config.data}
          title={commonProps.title}
          height="auto"
        />
      )

    case 'pie-chart':
      return (
        <PieChart
          {...spreadableProps}
          dataSource={config.dataSource}
          data={config.data}
          title={commonProps.title}
          height="auto"
        />
      )

    case 'donut-chart':
      return (
        <DonutChart
          {...spreadableProps}
          dataSource={config.dataSource}
          data={config.data || [
            { name: 'A', value: 30 },
            { name: 'B', value: 50 },
            { name: 'C', value: 20 }
          ]}
          title={commonProps.title}
          height="auto"
        />
      )

    case 'gauge-chart':
      return (
        <GaugeChart
          {...spreadableProps}
          dataSource={config.dataSource}
          value={config.value}
          min={config.min ?? 0}
          max={config.max ?? 100}
          label={commonProps.title || 'Gauge'}
          unit={config.unit}
        />
      )

    // Controls
    case 'toggle-switch':
      return (
        <ToggleSwitch
          {...spreadableProps}
          dataSource={config.dataSource}
          label={config.label || commonProps.title}
          checked={config.checked ?? false}
          onCheckedChange={config.onCheckedChange}
        />
      )

    case 'button-group':
      return (
        <ButtonGroup
          {...spreadableProps}
          options={config.options || [
            { label: 'Button 1', value: 'btn1' },
            { label: 'Button 2', value: 'btn2' }
          ]}
          value={config.value}
          onValueChange={config.onValueChange}
          orientation={config.orientation || 'horizontal'}
        />
      )

    case 'dropdown':
      return (
        <Dropdown
          {...spreadableProps}
          options={config.options || [
            { label: 'Option 1', value: 'opt1' },
            { label: 'Option 2', value: 'opt2' }
          ]}
          value={config.value}
          onValueChange={config.onValueChange}
          placeholder={config.placeholder || 'Select...'}
        />
      )

    case 'input-field':
      return (
        <InputField
          {...spreadableProps}
          value={config.value || ''}
          onValueChange={config.onValueChange}
          placeholder={config.placeholder}
          type={config.type || 'text'}
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
            variant="destructive"
            size="icon"
            className="h-7 w-7 bg-background/90 backdrop-blur"
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

  const handleDashboardDelete = useCallback((id: string) => {
    if (confirm('Delete this dashboard?')) {
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
          series: [{ name: 'Value', data: [10, 25, 15, 30, 28, 35, 20] }],
          labels: ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']
        }
        break
      case 'bar-chart':
        defaultConfig = {
          data: [{ name: 'A', value: 30 }, { name: 'B', value: 50 }, { name: 'C', value: 20 }]
        }
        break
      case 'pie-chart':
      case 'donut-chart':
        defaultConfig = {
          data: [{ name: 'A', value: 30 }, { name: 'B', value: 50 }, { name: 'C', value: 20 }]
        }
        break
      case 'gauge-chart':
        defaultConfig = {
          value: 65,
          min: 0,
          max: 100
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
          checked: true
        }
        break
      case 'button-group':
        defaultConfig = {
          options: [{ label: 'Start', value: 'start' }, { label: 'Stop', value: 'stop' }],
          value: 'start'
        }
        break
      case 'dropdown':
        defaultConfig = {
          options: [{ label: 'Option 1', value: 'opt1' }, { label: 'Option 2', value: 'opt2' }],
          value: 'opt1',
          placeholder: 'Select...'
        }
        break
      case 'input-field':
        defaultConfig = {
          value: '',
          type: 'text',
          placeholder: 'Enter value...'
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [componentsStableKey, editMode])

  // Track initial config load to avoid unnecessary updates
  const initialConfigRef = useRef<any>(null)
  const isInitialLoad = useRef(false)

  // Live preview: update component in real-time as config changes
  useEffect(() => {
    if (configOpen && selectedComponent) {
      // Skip initial load - don't update with same config
      if (!isInitialLoad.current) {
        initialConfigRef.current = componentConfig
        isInitialLoad.current = true
        setConfigSchema(generateConfigSchema(selectedComponent.type, componentConfig))
        return
      }

      // Only update if config actually changed
      const currentJSON = JSON.stringify(componentConfig)
      const initialJSON = JSON.stringify(initialConfigRef.current)
      if (currentJSON !== initialJSON) {
        // Update the component with current config for live preview
        updateComponent(selectedComponent.id, { config: componentConfig })
        // Regenerate schema with new config values
        setConfigSchema(generateConfigSchema(selectedComponent.type, componentConfig))
      }
    } else {
      // Reset when dialog closes
      isInitialLoad.current = false
      initialConfigRef.current = null
    }
  }, [componentConfig, configOpen, selectedComponent])

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
        return createDataDisplayConfig({
          dataSource: config.dataSource,
          onDataSourceChange: updateDataSource,
          unit: config.unit,
          onUnitChange: updateConfig('unit'),
          prefix: config.prefix,
          onPrefixChange: updateConfig('prefix'),
          suffix: config.suffix,
          onSuffixChange: updateConfig('suffix'),
          decimals: config.decimals,
          onDecimalsChange: updateConfig('decimals'),
          size: config.size,
          onSizeChange: updateConfig('size'),
          color: config.color,
          onColorChange: updateConfig('color'),
          showTrend: config.showTrend,
          onShowTrendChange: updateConfig('showTrend'),
          showChange: config.showChange,
          onShowChangeChange: updateConfig('showChange'),
        })

      case 'sparkline':
        return createChartConfig({
          dataSource: config.dataSource,
          onDataSourceChange: updateDataSource,
          label: config.label,
          onLabelChange: updateConfig('label'),
          showPoints: config.showPoints,
          onShowPointsChange: updateConfig('showPoints'),
        })

      case 'progress-bar':
        return createProgressConfig({
          dataSource: config.dataSource,
          onDataSourceChange: updateDataSource,
          label: config.label,
          onLabelChange: updateConfig('label'),
          value: config.value,
          onValueChange: updateConfig('value'),
          min: config.min,
          onMinChange: updateConfig('min'),
          max: config.max,
          onMaxChange: updateConfig('max'),
          color: config.color,
          onColorChange: updateConfig('color'),
        })

      case 'led-indicator':
        return createIndicatorConfig({
          dataSource: config.dataSource,
          onDataSourceChange: updateDataSource,
          state: config.state,
          onStateChange: updateConfig('state'),
          size: config.size,
          onSizeChange: updateConfig('size'),
          colors: {
            on: config.color,
            error: config.errorColor,
            warning: config.warningColor,
          },
          onColorChange: (key, color) => {
            if (key === 'on') updateConfig('color')(color)
            else if (key === 'error') updateConfig('errorColor')(color)
            else if (key === 'warning') updateConfig('warningColor')(color)
          },
        })

      // ========== Charts ==========
      case 'line-chart':
      case 'area-chart':
        return createChartConfig({
          dataSource: config.dataSource,
          onDataSourceChange: updateDataSource,
          showPoints: config.showPoints,
          onShowPointsChange: updateConfig('showPoints'),
        })

      case 'bar-chart':
        return createChartConfig({
          dataSource: config.dataSource,
          onDataSourceChange: updateDataSource,
          showLabels: config.showLabels,
          onShowLabelsChange: updateConfig('showLabels'),
        })

      case 'pie-chart':
      case 'donut-chart':
        return createChartConfig({
          dataSource: config.dataSource,
          onDataSourceChange: updateDataSource,
          showLabels: config.showLabels,
          onShowLabelsChange: updateConfig('showLabels'),
        })

      case 'gauge-chart':
        return {
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-4">
                  <div className="grid grid-cols-2 gap-2">
                    <div className="space-y-2">
                      <label className="text-sm font-medium">Value</label>
                      <input
                        type="number"
                        value={config.value ?? 0}
                        onChange={(e) => updateConfig('value')(parseFloat(e.target.value) || 0)}
                        className="w-full h-10 px-3 rounded-md border border-input bg-background"
                      />
                    </div>
                    <div className="space-y-2">
                      <label className="text-sm font-medium">Min</label>
                      <input
                        type="number"
                        value={config.min ?? 0}
                        onChange={(e) => updateConfig('min')(parseFloat(e.target.value) || 0)}
                        className="w-full h-10 px-3 rounded-md border border-input bg-background"
                      />
                    </div>
                    <div className="space-y-2">
                      <label className="text-sm font-medium">Max</label>
                      <input
                        type="number"
                        value={config.max ?? 100}
                        onChange={(e) => updateConfig('max')(parseFloat(e.target.value) || 100)}
                        className="w-full h-10 px-3 rounded-md border border-input bg-background"
                      />
                    </div>
                    <div className="space-y-2">
                      <label className="text-sm font-medium">Unit</label>
                      <input
                        type="text"
                        value={config.unit || ''}
                        onChange={(e) => updateConfig('unit')(e.target.value)}
                        className="w-full h-10 px-3 rounded-md border border-input bg-background"
                      />
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
        return createControlConfig({
          dataSource: config.dataSource,
          onDataSourceChange: updateDataSource,
          value: config.checked,
          onValueChange: updateConfig('checked'),
          size: config.size,
          onSizeChange: updateConfig('size'),
        })

      case 'button-group':
        return {
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-4">
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Orientation</label>
                    <select
                      value={config.orientation || 'horizontal'}
                      onChange={(e) => updateConfig('orientation')(e.target.value)}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    >
                      <option value="horizontal">Horizontal</option>
                      <option value="vertical">Vertical</option>
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
              },
            },
          ],
        }

      case 'dropdown':
        return {
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-4">
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Placeholder</label>
                    <input
                      type="text"
                      value={config.placeholder || ''}
                      onChange={(e) => updateConfig('placeholder')(e.target.value)}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    />
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

      case 'input-field':
        return {
          styleSections: [
            {
              type: 'custom' as const,
              render: () => (
                <div className="space-y-4">
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Input Type</label>
                    <select
                      value={config.type || 'text'}
                      onChange={(e) => updateConfig('type')(e.target.value)}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    >
                      <option value="text">Text</option>
                      <option value="email">Email</option>
                      <option value="password">Password</option>
                      <option value="tel">Phone</option>
                      <option value="url">URL</option>
                    </select>
                  </div>
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Placeholder</label>
                    <input
                      type="text"
                      value={config.placeholder || ''}
                      onChange={(e) => updateConfig('placeholder')(e.target.value)}
                      className="w-full h-10 px-3 rounded-md border border-input bg-background"
                    />
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
      />
    </div>
  )
}
