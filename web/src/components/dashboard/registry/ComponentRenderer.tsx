/**
 * Component Renderer
 *
 * Dynamic component rendering based on registry.
 * Uses component metadata to determine how to render each component type.
 */

import { lazy, Suspense } from 'react'
import { Card } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'
import type { DashboardComponent, GenericComponentType } from '@/types/dashboard'
import { getComponentMeta } from './registry'

// ============================================================================
// Lazy Import Components
// ============================================================================

// Indicators
const ValueCard = lazy(() => import('../generic/ValueCard').then(m => ({ default: m.ValueCard })))
const LEDIndicator = lazy(() => import('../generic/LEDIndicator').then(m => ({ default: m.LEDIndicator })))
const Sparkline = lazy(() => import('../generic/Sparkline').then(m => ({ default: m.Sparkline })))
const ProgressBar = lazy(() => import('../generic/ProgressBar').then(m => ({ default: m.ProgressBar })))

// Charts
const LineChart = lazy(() => import('../generic/LineChart').then(m => ({ default: m.LineChart })))
const AreaChart = lazy(() => import('../generic/LineChart').then(m => ({ default: m.AreaChart })))
const BarChart = lazy(() => import('../generic/BarChart').then(m => ({ default: m.BarChart })))
const PieChart = lazy(() => import('../generic/PieChart').then(m => ({ default: m.PieChart })))

// Controls
const ToggleSwitch = lazy(() => import('../generic/ToggleSwitch').then(m => ({ default: m.ToggleSwitch })))
const ButtonGroup = lazy(() => import('../generic/ButtonGroup').then(m => ({ default: m.ButtonGroup })))
const Slider = lazy(() => import('../generic/Slider').then(m => ({ default: m.Slider })))
const Dropdown = lazy(() => import('../generic/Dropdown').then(m => ({ default: m.Dropdown })))
const InputField = lazy(() => import('../generic/InputField').then(m => ({ default: m.InputField })))

// Lists & Tables
const DataTable = lazy(() => import('../generic/DataTable').then(m => ({ default: m.DataTable })))
const StatusList = lazy(() => import('../generic/StatusList').then(m => ({ default: m.StatusList })))
const LogFeed = lazy(() => import('../generic/LogFeed').then(m => ({ default: m.LogFeed })))

// Display & Content
const ImageDisplay = lazy(() => import('../generic/ImageDisplay').then(m => ({ default: m.ImageDisplay })))
const ImageHistory = lazy(() => import('../generic/ImageHistory').then(m => ({ default: m.ImageHistory })))
const WebDisplay = lazy(() => import('../generic/WebDisplay').then(m => ({ default: m.WebDisplay })))
const MarkdownDisplay = lazy(() => import('../generic/MarkdownDisplay').then(m => ({ default: m.MarkdownDisplay })))

// Business Components
const AgentStatusCard = lazy(() => import('../business/AgentStatusCard').then(m => ({ default: m.AgentStatusCard })))
const DecisionList = lazy(() => import('../business/DecisionList').then(m => ({ default: m.DecisionList })))
const DeviceControl = lazy(() => import('../business/DeviceControl').then(m => ({ default: m.DeviceControl })))
const RuleStatusGrid = lazy(() => import('../business/RuleStatusGrid').then(m => ({ default: m.RuleStatusGrid })))
const TransformList = lazy(() => import('../business/TransformList').then(m => ({ default: m.TransformList })))

// ============================================================================
// Component Map
// ============================================================================

const componentMap: Record<GenericComponentType, React.ComponentType<any>> = {
  // Indicators
  'value-card': ValueCard,
  'led-indicator': LEDIndicator,
  'sparkline': Sparkline,
  'progress-bar': ProgressBar,

  // Charts
  'line-chart': LineChart,
  'area-chart': AreaChart,
  'bar-chart': BarChart,
  'pie-chart': PieChart,

  // Controls
  'toggle-switch': ToggleSwitch,
  'button-group': ButtonGroup,
  'slider': Slider,
  'dropdown': Dropdown,
  'input-field': InputField,

  // Lists & Tables
  'data-table': DataTable,
  'status-list': StatusList,
  'log-feed': LogFeed,

  // Display & Content
  'image-display': ImageDisplay,
  'image-history': ImageHistory,
  'web-display': WebDisplay,
  'markdown-display': MarkdownDisplay,
} as const

// Business component map
const businessComponentMap = {
  'agent-status-card': AgentStatusCard,
  'decision-list': DecisionList,
  'device-control': DeviceControl,
  'rule-status-grid': RuleStatusGrid,
  'transform-list': TransformList,
} as const

// ============================================================================
// Loading Skeleton
// ============================================================================

interface ComponentSkeletonProps {
  meta: ReturnType<typeof getComponentMeta>
  className?: string
}

function ComponentSkeleton({ meta, className }: ComponentSkeletonProps) {
  if (!meta) {
    return <Skeleton className={cn('w-full h-48', className)} />
  }

  const { sizeConstraints } = meta
  const minHeight = sizeConstraints.minH * 40 // Approximate grid row height

  return (
    <Skeleton
      className={cn('w-full', className)}
      style={{ minHeight: `${minHeight}px` }}
    />
  )
}

// ============================================================================
// Fallback for Unknown Component
// ============================================================================

interface UnknownComponentProps {
  type: string
  className?: string
}

function UnknownComponent({ type, className }: UnknownComponentProps) {
  return (
    <Card className={cn('border-dashed border-2', className)}>
      <div className="flex items-center justify-center h-full min-h-[120px] p-4 text-center">
        <div className="text-muted-foreground">
          <p className="font-medium">Unknown Component</p>
          <p className="text-sm text-muted-foreground/60 mt-1">
            Type: <code className="text-xs bg-muted px-1 py-0.5 rounded">{type}</code>
          </p>
        </div>
      </div>
    </Card>
  )
}

// ============================================================================
// Main Renderer
// ============================================================================

export interface RenderComponentProps {
  component: DashboardComponent
  className?: string
  style?: React.CSSProperties
  onError?: (error: Error) => void
}

/**
 * Render a dashboard component based on its type
 * Uses the component registry to determine the appropriate component to render
 */
export default function ComponentRenderer({
  component,
  className,
  style,
  onError,
}: RenderComponentProps) {
  const meta = getComponentMeta(component.type)

  // Handle unknown component types
  if (!meta) {
    return <UnknownComponent type={component.type} className={className} />
  }

  const Component = componentMap[component.type as GenericComponentType] ||
                    businessComponentMap[component.type as keyof typeof businessComponentMap]

  if (!Component) {
    return <UnknownComponent type={component.type} className={className} />
  }

  // Extract common props from component config
  const config = (component as any).config || {}
  const dataSource = (component as any).dataSource
  const display = (component as any).display

  // Build props for the component
  const props = {
    key: component.id,
    dataSource,
    ...config,
    ...display,
    title: component.title || config.title,
    className: cn(
      'w-full h-full',
      className
    ),
    style,
  }

  return (
    <Suspense fallback={<ComponentSkeleton meta={meta} className={className} />}>
      <Component {...props} />
    </Suspense>
  )
}

/**
 * Render multiple components
 */
export interface RenderComponentsProps {
  components: DashboardComponent[]
  className?: string
  onComponentError?: (componentId: string, error: Error) => void
}

export function RenderComponents({
  components,
  className,
  onComponentError,
}: RenderComponentsProps) {
  return (
    <>
      {components.map(component => (
        <ComponentRenderer
          key={component.id}
          component={component}
          className={className}
          onError={(error) => onComponentError?.(component.id, error)}
        />
      ))}
    </>
  )
}
