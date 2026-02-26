/**
 * Component Renderer
 *
 * Dynamic component rendering based on registry.
 * Uses component metadata to determine how to render each component type.
 * Supports both static (built-in) and dynamic (extension-provided) components.
 */

import { lazy, Suspense, memo, useMemo, useState, useEffect } from 'react'
import { Card } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'
import type { DashboardComponent, GenericComponentType } from '@/types/dashboard'
import { getComponentMeta } from './registry'
import { dynamicRegistry, dtoToComponentMeta } from './DynamicRegistry'

// ============================================================================
// Lazy Import Components
// ============================================================================

// Indicators
const ValueCard = lazy(() => import('../generic/ValueCard').then(m => ({ default: m.ValueCard })))
const LEDIndicator = lazy(() => import('../generic/LEDIndicator').then(m => ({ default: m.LEDIndicator })))
const Sparkline = lazy(() => import('../generic/Sparkline').then(m => ({ default: m.Sparkline })))
const ProgressBar = lazy(() => import('../generic/ProgressBar').then(m => ({ default: m.ProgressBar })))
const AgentStatusCard = lazy(() => import('../generic/AgentStatusCard').then(m => ({ default: m.AgentStatusCard })))

// Charts
const LineChart = lazy(() => import('../generic/LineChart').then(m => ({ default: m.LineChart })))
const AreaChart = lazy(() => import('../generic/LineChart').then(m => ({ default: m.AreaChart })))
const BarChart = lazy(() => import('../generic/BarChart').then(m => ({ default: m.BarChart })))
const PieChart = lazy(() => import('../generic/PieChart').then(m => ({ default: m.PieChart })))

// Controls
const ToggleSwitch = lazy(() => import('../generic/ToggleSwitch').then(m => ({ default: m.ToggleSwitch })))

// Display & Content
const ImageDisplay = lazy(() => import('../generic/ImageDisplay').then(m => ({ default: m.ImageDisplay })))
const ImageHistory = lazy(() => import('../generic/ImageHistory').then(m => ({ default: m.ImageHistory })))
const WebDisplay = lazy(() => import('../generic/WebDisplay').then(m => ({ default: m.WebDisplay })))
const MarkdownDisplay = lazy(() => import('../generic/MarkdownDisplay').then(m => ({ default: m.MarkdownDisplay })))

// Spatial & Media
const MapDisplay = lazy(() => import('../generic/MapDisplay').then(m => ({ default: m.MapDisplay })))
const VideoDisplay = lazy(() => import('../generic/VideoDisplay').then(m => ({ default: m.VideoDisplay })))
const CustomLayer = lazy(() => import('../generic/CustomLayer').then(m => ({ default: m.CustomLayer })))

// ============================================================================
// Component Map
// ============================================================================

const componentMap: Record<GenericComponentType, React.ComponentType<any>> = {
  // Indicators
  'value-card': ValueCard,
  'led-indicator': LEDIndicator,
  'sparkline': Sparkline,
  'progress-bar': ProgressBar,
  'agent-status-card': AgentStatusCard,

  // Charts
  'line-chart': LineChart,
  'area-chart': AreaChart,
  'bar-chart': BarChart,
  'pie-chart': PieChart,

  // Controls
  'toggle-switch': ToggleSwitch,

  // Display & Content
  'image-display': ImageDisplay,
  'image-history': ImageHistory,
  'web-display': WebDisplay,
  'markdown-display': MarkdownDisplay,

  // Spatial & Media
  'map-display': MapDisplay,
  'video-display': VideoDisplay,
  'custom-layer': CustomLayer,
} as const

// ============================================================================
// Business Components Map
// ============================================================================

const AgentMonitorWidget = lazy(() => import('../generic/AgentMonitorWidget').then(m => ({ default: m.AgentMonitorWidget })))

const businessComponentMap: Record<string, React.ComponentType<any>> = {
  'agent-monitor-widget': AgentMonitorWidget,
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
 * Supports both static (built-in) and dynamic (extension-provided) components
 *
 * Memoized to prevent unnecessary re-renders when parent updates.
 * Only re-renders when component.id, component.type, component.dataSource,
 * component.config, component.title, component.display, className, or style changes.
 */
const ComponentRenderer = memo(function ComponentRenderer({
  component,
  className,
  style,
  onError,
}: RenderComponentProps) {
  const componentType = component.type
  const isDynamic = dynamicRegistry.isDynamic(componentType)

  // State for dynamic component loading
  const [DynamicComponent, setDynamicComponent] = useState<React.ComponentType<any> | null>(null)
  const [loading, setLoading] = useState(false)
  const [loadError, setLoadError] = useState<Error | null>(null)

  // Load dynamic component when type changes
  useEffect(() => {
    if (!isDynamic) {
      setDynamicComponent(null)
      setLoadError(null)
      return
    }

    setLoading(true)
    setLoadError(null)

    dynamicRegistry.loadComponent(componentType)
      .then((module) => {
        if (module) {
          // Handle different export formats
          // - function: regular component
          // - object with $$typeof: forwardRef, memo, etc.
          // - object with default: module default export
          let Component: React.ComponentType<any> | null = null

          if (typeof module === 'function') {
            Component = module as React.ComponentType
          } else if (typeof module === 'object' && module !== null) {
            // Check if it's a forwardRef/memo component
            if ((module as any).$$typeof || typeof (module as any).render === 'function') {
              Component = module as React.ComponentType
            } else if ('default' in module) {
              // Module default export
              const defaultExport = (module as { default: unknown }).default
              if (typeof defaultExport === 'function') {
                Component = defaultExport as React.ComponentType
              } else if (typeof defaultExport === 'object' && defaultExport !== null) {
                // Default export might be forwardRef/memo
                if ((defaultExport as any).$$typeof || typeof (defaultExport as any).render === 'function') {
                  Component = defaultExport as React.ComponentType
                }
              }
            }
          }

          if (Component) {
            setDynamicComponent(() => Component as React.ComponentType)
          } else {
            setLoadError(new Error(`Failed to load component: ${componentType} (invalid component type)`))
          }
        } else {
          setLoadError(new Error(`Failed to load component: ${componentType}`))
        }
      })
      .catch((err) => {
        console.error(`[ComponentRenderer] loadComponent error for ${componentType}:`, err)
        setLoadError(err)
      })
      .finally(() => {
        setLoading(false)
      })
  }, [componentType, isDynamic])

  // Get metadata (check both static and dynamic registries)
  let meta = getComponentMeta(componentType)

  // If not found in static registry, try dynamic registry
  if (!meta && isDynamic) {
    const dto = dynamicRegistry.getMeta(componentType)
    if (dto) {
      meta = dtoToComponentMeta(dto)
    }
  }

  // Try to get component from generic or business component map (static components)
  const StaticComponent = componentMap[component.type as GenericComponentType] || businessComponentMap[component.type]

  // Determine which component to render
  const Component = isDynamic ? DynamicComponent : StaticComponent

  // Extract specific values for stable dependencies
  // Use individual properties instead of entire component object to prevent unnecessary re-creates
  const componentId = component.id
  const componentTitle = component.title
  const componentConfig = (component as any).config || {}
  const componentDataSource = (component as any).dataSource
  const componentDisplay = (component as any).display

  // Memoize props to prevent unnecessary re-renders of child components
  // Only recreate when actual component data changes
  // IMPORTANT: Must be before any early returns to follow React Hooks rules
  const props = useMemo(() => {
    const { editMode, ...restConfig } = componentConfig

    // Build props for the component (NOT including key - key must be passed directly)
    const builtProps: Record<string, any> = {
      dataSource: componentDataSource,
      editMode, // Pass editMode as a separate prop for components that need it
      ...restConfig,
      ...componentDisplay,
      title: componentTitle || componentConfig.title,
      className: cn(
        'w-full h-full',
        className
      ),
      style,
    }

    // Special handling for agent-monitor-widget: extract agentId from dataSource
    if (componentType === 'agent-monitor-widget' && componentDataSource?.agentId) {
      builtProps.agentId = componentDataSource.agentId
    }

    return builtProps
  }, [componentId, componentType, componentTitle, componentConfig, componentDataSource, componentDisplay, className, style])

  // Show loading state for dynamic components
  if (isDynamic && loading) {
    return <ComponentSkeleton meta={meta} className={className} />
  }

  // Show error state for dynamic component load failures
  if (isDynamic && loadError) {
    return (
      <Card className={cn('border-destructive/50', className)}>
        <div className="flex items-center justify-center h-full min-h-[120px] p-4 text-center">
          <div className="text-destructive">
            <p className="font-medium">Component Load Failed</p>
            <p className="text-sm text-muted-foreground mt-1">
              {loadError.message}
            </p>
          </div>
        </div>
      </Card>
    )
  }

  // Handle unknown component types (after hooks to follow React Hooks rules)
  if (!meta || !Component) {
    return <UnknownComponent type={component.type} className={className} />
  }

  return (
    <Suspense fallback={<ComponentSkeleton meta={meta} className={className} />}>
      <Component key={component.id} {...props} />
    </Suspense>
  )
}, (prevProps, nextProps) => {
  // Custom comparison for more precise re-render control
  const prevComp = prevProps.component as any
  const nextComp = nextProps.component as any

  // Quick primitive checks
  if (prevProps.component.id !== nextProps.component.id) return false
  if (prevProps.component.type !== nextProps.component.type) return false
  if (prevProps.component.title !== nextProps.component.title) return false
  if (prevProps.className !== nextProps.className) return false
  if (prevProps.style !== nextProps.style) return false

  // Helper for stable deep comparison of objects
  const deepEqual = (a: unknown, b: unknown): boolean => {
    // Reference equality
    if (a === b) return true
    // Both null/undefined
    if (a == null || b == null) return a === b
    // Type mismatch
    if (typeof a !== typeof b) return false
    // One is array, other isn't
    if (Array.isArray(a) !== Array.isArray(b)) return false

    // For arrays and objects, use JSON.stringify with sorted keys
    // This handles property order differences
    try {
      return JSON.stringify(a) === JSON.stringify(b)
    } catch {
      // Fallback for circular references or non-serializable values
      return false
    }
  }

  // Deep compare complex objects
  if (!deepEqual(prevComp.dataSource, nextComp.dataSource)) return false
  if (!deepEqual(prevComp.config, nextComp.config)) return false
  if (!deepEqual(prevComp.display, nextComp.display)) return false

  // All checks passed - props are equal
  return true
})

export default ComponentRenderer

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
