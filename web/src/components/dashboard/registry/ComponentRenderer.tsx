/**
 * Component Renderer
 *
 * Dynamic component rendering based on registry.
 * Uses component metadata to determine how to render each component type.
 * Supports both static (built-in) and dynamic (extension-provided) components.
 */

import { lazy, Suspense, memo, useMemo, useState, useEffect, useCallback, useRef } from 'react'
import { AlertTriangle } from 'lucide-react'
import { Card } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'
import { ErrorBoundary } from '@/components/shared/ErrorBoundary'
import { findDevice } from '@/lib/deviceUtils'
import type { DataSource } from '@/types/dashboard'
import { normalizeDataSource } from '@/types/dashboard'
import { resolveComponentData } from '@/lib/componentDataApi'
import type { DashboardComponent, GenericComponentType } from '@/types/dashboard'
import type { Device, DeviceType } from '@/types'
import { useStore } from '@/store'
import { useEvents } from '@/hooks/useEvents'
import { getComponentMeta } from './registry'
import { dynamicRegistry, dtoToComponentMeta } from './DynamicRegistry'
import { communityRegistry } from './CommunityRegistry'

// ============================================================================
// Static imports — avoids lazy() + Suspense fragility in WKWebView
// ============================================================================

// Indicators
import { ValueCard } from '../generic/ValueCard'
import { LEDIndicator } from '../generic/LEDIndicator'
import { Sparkline } from '../generic/Sparkline'
import { ProgressBar } from '../generic/ProgressBar'

// Charts
import { LineChart } from '../generic/LineChart'
import { AreaChart } from '../generic/LineChart'
import { BarChart } from '../generic/BarChart'
import { PieChart } from '../generic/PieChart'

// Controls
import { CommandButton } from '../generic/CommandButton'

// Display & Content
import { ImageDisplay } from '../generic/ImageDisplay'
import { ImageHistory } from '../generic/ImageHistory'
import { WebDisplay } from '../generic/WebDisplay'
import { MarkdownDisplay } from '../generic/MarkdownDisplay'

// Spatial & Media
import { MapDisplay } from '../generic/MapDisplay'
import { VideoDisplay } from '../generic/VideoDisplay'
import { CustomLayer } from '../generic/CustomLayer'

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
  'toggle-switch': CommandButton,

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
const AiAnalyst = lazy(() => import('../generic/AiAnalyst').then(m => ({ default: m.AiAnalyst })))

const businessComponentMap: Record<string, React.ComponentType<any>> = {
  'agent-monitor-widget': AgentMonitorWidget,
  'ai-analyst': AiAnalyst,
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
    return <Skeleton className={cn('w-full h-full', className)} />
  }

  return (
    <Skeleton
      className={cn('w-full h-full', className)}
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
          <p className="text-sm text-muted-foreground mt-1">
            Type: <code className="text-xs bg-muted px-1 py-0.5 rounded">{type}</code>
          </p>
        </div>
      </div>
    </Card>
  )
}

// ============================================================================
// Error Fallback — shown when a component crashes during render
// Isolates the error to a single grid cell so the rest of the dashboard
// stays functional. User can still access config to fix/remove the component.
// ============================================================================

interface ComponentErrorFallbackProps {
  className?: string
}

function ComponentErrorFallback({ className }: ComponentErrorFallbackProps) {
  return (
    <Card className={cn('border-error-light', className)}>
      <div className="flex flex-col items-center justify-center h-full min-h-[120px] p-4 text-center">
        <div className="w-8 h-8 rounded-full bg-destructive-light flex items-center justify-center mb-2">
          <AlertTriangle className="h-4 w-4 text-error" />
        </div>
        <p className="text-xs font-medium text-error">Component Error</p>
        <p className="text-nano text-muted-foreground mt-1">Check config or remove this component</p>
      </div>
    </Card>
  )
}

// ============================================================================
// Deep Equal Utility (module-level to avoid re-allocation on every comparison)
// ============================================================================

// Recursive shallow comparison — avoids JSON.stringify GC pressure
// for 20+ components on every parent re-render.
const deepEqual = (a: unknown, b: unknown): boolean => {
  if (a === b) return true
  if (a == null || b == null) return a === b
  if (typeof a !== typeof b) return false
  if (Array.isArray(a) !== Array.isArray(b)) return false
  if (Array.isArray(a)) {
    if (a.length !== (b as unknown[]).length) return false
    for (let i = 0; i < a.length; i++) {
      if (!deepEqual(a[i], (b as unknown[])[i])) return false
    }
    return true
  }
  if (typeof a === 'object') {
    const aObj = a as Record<string, unknown>
    const bObj = b as Record<string, unknown>
    const keysA = Object.keys(aObj)
    const keysB = Object.keys(bObj)
    if (keysA.length !== keysB.length) return false
    for (const key of keysA) {
      if (!deepEqual(aObj[key], bObj[key])) return false
    }
    return true
  }
  return a === b
}

// ============================================================================
// Main Renderer
// ============================================================================

/**
 * Device metric type for extension components
 */
export interface DeviceMetric {
  id: string
  name: string
  type?: string
  data_type?: string
  value?: unknown
  timestamp?: number
}

/**
 * Device type for extension components
 */
export interface DeviceForExtension {
  id: string
  name: string
  type?: string
  metrics?: DeviceMetric[]
}

export interface RenderComponentProps {
  component: DashboardComponent
  className?: string
  style?: React.CSSProperties
  onError?: (error: Error) => void
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  onDataSourceChange?: (dataSource: any) => void
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  onConfigChange?: (config: any) => void
  /** Open a fullscreen dialog with arbitrary React content (for extension components) */
  openFullscreen?: (content: React.ReactNode) => void
  /** Close the fullscreen dialog */
  closeFullscreen?: () => void
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
  onDataSourceChange,
  onConfigChange,
  openFullscreen,
  closeFullscreen,
}: RenderComponentProps) {
  const componentType = component.type
  // Built-in types take priority — community components may share the same ID
  // (e.g., a community "toggle-switch" should NOT override the built-in one)
  const isBuiltIn = !!(componentMap[componentType as GenericComponentType] || businessComponentMap[componentType])
  const isDynamic = !isBuiltIn && dynamicRegistry.isDynamic(componentType)
  const isCommunity = !isBuiltIn && communityRegistry.isCommunity(componentType)

  // State for dynamic component loading
  const [DynamicComponent, setDynamicComponent] = useState<React.ComponentType<any> | null>(null)
  const [loading, setLoading] = useState(false)
  const [loadError, setLoadError] = useState<Error | null>(null)
  const [attemptCount, setAttemptCount] = useState(0)
  const [registrationPollCount, setRegistrationPollCount] = useState(0)

  // Heuristic: check if this looks like an extension component (not in any registry)
  const isUnknownType = !isBuiltIn
  const mightBeExtension = isUnknownType && !isDynamic && !isCommunity

  // Max auto-retry attempts
  const MAX_LOAD_RETRIES = 5
  const LOAD_RETRY_DELAY = 800
  const MAX_REGISTRATION_POLLS = 20
  const REGISTRATION_POLL_INTERVAL = 200

  // Load dynamic component with auto-retry
  const loadDynamicComponent = useCallback(async (attempt: number): Promise<void> => {
    if (!isDynamic && !isCommunity) {
      setDynamicComponent(null)
      setLoadError(null)
      return
    }

    setLoading(true)
    setLoadError(null)

    try {
      const module = isCommunity
        ? await communityRegistry.loadComponent(componentType)
        : await dynamicRegistry.loadComponent(componentType)

      if (module) {
        let Component: React.ComponentType<any> | null = null

        if (typeof module === 'function') {
          Component = module as React.ComponentType
        } else if (typeof module === 'object' && module !== null) {
          if ((module as any).$$typeof || typeof (module as any).render === 'function') {
            Component = module as React.ComponentType
          } else if ('default' in module) {
            const defaultExport = (module as { default: unknown }).default
            if (typeof defaultExport === 'function') {
              Component = defaultExport as React.ComponentType
            } else if (typeof defaultExport === 'object' && defaultExport !== null) {
              if ((defaultExport as any).$$typeof || typeof (defaultExport as any).render === 'function') {
                Component = defaultExport as React.ComponentType
              }
            }
          }
        }

        if (Component) {
          setDynamicComponent(() => Component as React.ComponentType)
          setLoadError(null)
        } else {
          throw new Error(`Invalid component type`)
        }
      } else {
        throw new Error(`Module not found`)
      }
    } catch (err) {
      console.error(`[ComponentRenderer] Load attempt ${attempt} failed for ${componentType}:`, err)

      // Auto-retry if we haven't exceeded max attempts
      if (attempt < MAX_LOAD_RETRIES) {
        const retryType = componentType // capture current type
        setTimeout(() => {
          // Only retry if the component type hasn't changed during the delay
          setAttemptCount(prev => prev === attempt ? attempt + 1 : prev)
        }, LOAD_RETRY_DELAY)
      } else {
        setLoadError(err instanceof Error ? err : new Error(String(err)))
      }
    } finally {
      setLoading(false)
    }
  }, [componentType, isDynamic, isCommunity])

  // Trigger load when isDynamic or isCommunity changes or retry is needed
  useEffect(() => {
    if (isDynamic || isCommunity) {
      loadDynamicComponent(attemptCount)
    }
  }, [isDynamic, isCommunity, attemptCount, loadDynamicComponent])

  // Reset attempt count when component type changes
  useEffect(() => {
    setAttemptCount(0)
    setDynamicComponent(null)
    setLoadError(null)
    setRegistrationPollCount(0)
  }, [componentType])

  // Wait for extension components to be registered (polling)
  useEffect(() => {
    if (!mightBeExtension) return

    // Check immediately
    if (dynamicRegistry.isDynamic(componentType)) {
      return
    }

    // Poll for component registration
    const pollInterval = setInterval(() => {
      setRegistrationPollCount(prev => {
        const next = prev + 1
        if (next >= MAX_REGISTRATION_POLLS) {
          clearInterval(pollInterval)
        }
        return next
      })
    }, REGISTRATION_POLL_INTERVAL)

    return () => clearInterval(pollInterval)
  }, [mightBeExtension, componentType])

  // Subscribe to installed community component count to detect when
  // communityRegistry gets populated after async fetchInstalled().
  // This breaks through the memo boundary so isCommunity is re-evaluated.
  const communityRevision = useStore(useCallback((s: any) =>
    (s.installed?.length ?? 0) + (s.loading ? 0.5 : 0)
  , []))

  // Re-check isDynamic when poll count changes
  const currentIsDynamic = dynamicRegistry.isDynamic(componentType)
  const currentIsCommunity = communityRegistry.isCommunity(componentType)

  // Trigger load when component becomes registered (or community registry populates)
  useEffect(() => {
    if ((currentIsDynamic || currentIsCommunity) && !isDynamic && !isCommunity && !DynamicComponent && !loading && !loadError) {
      // Component just became registered, trigger load
      setAttemptCount(0)
    }
  }, [currentIsDynamic, currentIsCommunity, isDynamic, isCommunity, DynamicComponent, loading, loadError, communityRevision])

  // Get metadata (check static, dynamic, and community registries)
  let meta = getComponentMeta(componentType)

  // If not found in static registry, try dynamic registry
  if (!meta && isDynamic) {
    const dto = dynamicRegistry.getMeta(componentType)
    if (dto) {
      meta = dtoToComponentMeta(dto)
    }
  }

  // If not found, try community registry
  if (!meta && isCommunity) {
    const cMeta = communityRegistry.getMeta(componentType)
    if (cMeta) {
      meta = communityRegistry.communityMetaToComponentMeta(cMeta)
    }
  }

  // Try to get component from generic or business component map (static components)
  const StaticComponent = componentMap[component.type as GenericComponentType] || businessComponentMap[component.type]

  // Determine which component to render
  const Component = (isDynamic || isCommunity) ? DynamicComponent : StaticComponent

  // Extract specific values for stable dependencies
  // Use individual properties instead of entire component object to prevent unnecessary re-creates
  const componentId = component.id
  const componentTitle = component.title
  const componentConfig = (component as any).config || {}
  const componentDataSource = (component as any).dataSource
  const componentDisplay = (component as any).display

  // Device binding: check if component has device binding config
  const boundDeviceId = componentConfig.deviceBinding?.deviceId as string | undefined
  const communityMetaForDevice = isCommunity ? communityRegistry.getMeta(componentType) : null
  const dynamicMetaForDevice = isDynamic ? dynamicRegistry.getMeta(componentType) : null
  const hasDeviceBinding = !!(
    (communityMetaForDevice?.has_device_binding || dynamicMetaForDevice?.has_device_binding) && boundDeviceId
  )

  // Subscribe to bound device from store (static config only — does NOT change on telemetry updates)
  const boundDevice = useStore(useCallback((s: any) =>
    hasDeviceBinding ? findDevice(s.devices, boundDeviceId) : null
  , [hasDeviceBinding, boundDeviceId]))

  // Subscribe to bound device's telemetry independently (high-frequency updates)
  const boundDeviceTelemetry = useStore(useCallback((s: any) =>
    hasDeviceBinding && boundDeviceId ? s.deviceTelemetry[boundDeviceId] : undefined
  , [hasDeviceBinding, boundDeviceId]))

  const boundDeviceType = useStore(useCallback((s: any) =>
    hasDeviceBinding && boundDevice
      ? s.deviceTypes.find((dt: DeviceType) => dt.device_type === boundDevice.device_type)
      : null
  , [hasDeviceBinding, boundDevice]))

  const sendCommand = useStore((s) => s.sendCommand)

  // Build deviceContext and sendDeviceCommand for bound components
  const deviceContext = useMemo(() => {
    if (!hasDeviceBinding || !boundDevice) return undefined
    // Read telemetry from split map, fallback to device.current_values
    const currentValues = boundDeviceTelemetry || boundDevice.current_values || {}
    return {
      device: {
        id: boundDevice.id,
        name: boundDevice.name,
        deviceType: boundDevice.device_type,
        status: boundDevice.online ? 'online' : 'offline',
        lastSeen: boundDevice.last_seen,
        currentValues,
      },
      deviceType: boundDeviceType ? {
        name: boundDeviceType.name,
        deviceType: boundDeviceType.device_type,
        metrics: boundDeviceType.metrics || [],
        commands: boundDeviceType.commands || [],
      } : undefined,
    }
  }, [hasDeviceBinding, boundDevice, boundDeviceType, boundDeviceTelemetry])

  const sendDeviceCommand = useMemo(() => {
    if (!hasDeviceBinding || !boundDeviceId) return undefined
    return async (command: string, params?: Record<string, unknown>) => {
      return sendCommand(boundDeviceId, command, params)
    }
  }, [hasDeviceBinding, boundDeviceId, sendCommand])

  // Subscribe to real-time device metric events for device-bound components
  // This ensures store.updateDeviceMetric is called even when no useDataSource hook
  // is active on the dashboard (community components don't use useDataSource)
  const processedEventsRef = useRef(new Set<string>())
  // Clear processed events when bound device changes to avoid stale dedup
  const prevBoundDeviceRef = useRef(boundDeviceId)
  if (prevBoundDeviceRef.current !== boundDeviceId) {
    prevBoundDeviceRef.current = boundDeviceId
    processedEventsRef.current.clear()
  }
  useEvents({
    enabled: hasDeviceBinding && !!boundDeviceId,
    category: 'device',
    onEvent: useCallback((event: any) => {
      if (!hasDeviceBinding || !boundDeviceId) return
      const eventData = event.data || event
      const eventType = event.type || eventData.type || ''
      const deviceId = eventData.device_id

      // Only process events for our bound device
      if (deviceId !== boundDeviceId) return

      // Deduplicate
      const eventId = eventData.id || `${eventType}-${eventData.timestamp}-${deviceId}`
      if (processedEventsRef.current.has(eventId)) return
      processedEventsRef.current.add(eventId)
      if (processedEventsRef.current.size > 100) {
        const entries = Array.from(processedEventsRef.current)
        processedEventsRef.current = new Set(entries.slice(-50))
      }

      // Check if this is a device metric event
      const normalized = eventType?.toLowerCase().replace('.', '')
      if (normalized?.includes('devicemetric') || normalized?.includes('metric') || eventType === 'DeviceMetric') {
        const store = useStore.getState()
        // Update primary metric value
        if ('metric' in eventData && 'value' in eventData) {
          store.updateDeviceMetric(deviceId, eventData.metric as string, eventData.value)
          // When device sends all telemetry as a single _raw JSON string,
          // parse and store each field individually so components can read
          // flat keys like "ts", "values.battery", "values.image"
          if (eventData.metric === '_raw' && typeof eventData.value === 'string') {
            try {
              const parsed = JSON.parse(eventData.value)
              if (parsed && typeof parsed === 'object') {
                for (const [key, val] of Object.entries(parsed)) {
                  if (val !== null && val !== undefined) {
                    store.updateDeviceMetric(deviceId, key, val)
                  }
                }
              }
            } catch { /* not JSON, ignore */ }
          }
        }
        // Update all other keys as nested metrics
        for (const [key, value] of Object.entries(eventData)) {
          if (key !== 'device_id' && key !== 'timestamp' && key !== 'type' && key !== 'id' && key !== 'metric' && key !== 'value') {
            store.updateDeviceMetric(deviceId, key, value)
          }
        }
      }
    }, [hasDeviceBinding, boundDeviceId]),
  })

  // Memoize props to prevent unnecessary re-renders of child components
  // Only recreate when actual component data changes
  // IMPORTANT: Must be before any early returns to follow React Hooks rules
  const props = useMemo(() => {
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    const { editMode, key: _key, ref: _ref, children: _children, ...restConfig } = componentConfig

    // Build props for the component (NOT including key - key must be passed directly)
    const builtProps: Record<string, any> = {
      dataSource: componentDataSource,
      editMode, // Pass editMode as a separate prop for components that need it
      config: componentConfig, // Pass full config object for community/extension components that read props.config
      ...restConfig,
      ...componentDisplay,
      title: componentTitle || componentConfig.title,
      className: cn(
        'w-full h-full',
        className
      ),
      style,
      // Pass callbacks for components to persist their configuration
      onDataSourceChange,
      onConfigChange,
      // Fullscreen dialog callbacks for extension components
      openFullscreen,
      closeFullscreen,
    }

    // Special handling for agent-monitor-widget: extract agentId from dataSource
    if (componentType === 'agent-monitor-widget' && componentDataSource?.agentId) {
      builtProps.agentId = componentDataSource.agentId
    }

    // ai-analyst: agentId comes from config (restConfig), no special handling needed

    // Device-bound community components get deviceContext + sendDeviceCommand
    if (deviceContext) {
      builtProps.deviceContext = deviceContext
    }
    if (sendDeviceCommand) {
      builtProps.sendDeviceCommand = sendDeviceCommand
    }

    // Community/extension components get fetchData for unified data access
    if (isDynamic || isCommunity) {
      builtProps.fetchData = async (options?: { timeRange?: number; limit?: number }) => {
        if (!componentDataSource) return null
        return resolveComponentData(componentDataSource as DataSource | DataSource[], options)
      }
    }

    return builtProps
  }, [componentId, componentType, componentTitle, componentConfig, componentDataSource, componentDisplay, className, style, onDataSourceChange, onConfigChange, deviceContext, sendDeviceCommand, openFullscreen, closeFullscreen])

  // Show loading state for dynamic/community components
  if ((isDynamic || isCommunity) && loading) {
    return <ComponentSkeleton meta={meta} className={className} />
  }

  // Show loading state for components that might be extension components but not yet registered
  if (mightBeExtension && registrationPollCount < MAX_REGISTRATION_POLLS) {
    return <ComponentSkeleton meta={undefined} className={className} />
  }

  // Show error state for dynamic/community component load failures (only after all retries exhausted)
  if ((isDynamic || isCommunity) && loadError && attemptCount >= MAX_LOAD_RETRIES) {
    return (
      <Card className={cn('border-error', className)}>
        <div className="flex items-center justify-center h-full min-h-[120px] p-4 text-center">
          <div className="text-error">
            <p className="font-medium">Component Load Failed</p>
            <p className="text-sm text-muted-foreground mt-1">
              {loadError.message}
            </p>
            <button
              onClick={() => {
                setLoadError(null)
                setAttemptCount(0)
              }}
              className="mt-2 px-3 py-1 text-xs hover:bg-muted rounded hover:bg-muted transition-colors"
            >
              Retry
            </button>
          </div>
        </div>
      </Card>
    )
  }

  // Show loading while retrying
  if ((isDynamic || isCommunity) && loadError && attemptCount < MAX_LOAD_RETRIES) {
    return <ComponentSkeleton meta={meta} className={className} />
  }

  // Handle unknown component types (after hooks to follow React Hooks rules)
  if (!meta || !Component) {
    return <UnknownComponent type={component.type} className={className} />
  }

  // ErrorBoundary resetKey: when config (e.g., deviceBinding) or component identity changes,
  // reset the error state so the component gets a fresh retry.
  const errorResetKey = component.id + ':' + component.type + ':' + (componentConfig.deviceBinding?.deviceId || '')

  // Built-in components: render directly without Suspense (they're statically imported)
  if (isBuiltIn) {
    return (
      <ErrorBoundary resetKey={errorResetKey} fallback={<ComponentErrorFallback className={className} />}>
        <Component key={component.id} {...props} />
      </ErrorBoundary>
    )
  }

  return (
    <ErrorBoundary resetKey={errorResetKey} fallback={<ComponentErrorFallback className={className} />}>
      <Suspense fallback={<ComponentSkeleton meta={meta} className={className} />}>
        <Component key={component.id} {...props} />
      </Suspense>
    </ErrorBoundary>
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
