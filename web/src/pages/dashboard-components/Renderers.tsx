/**
 * Dashboard Renderers
 *
 * BuiltInComponent, renderDashboardComponent, ComponentWrapper.
 * Extracted from VisualDashboard.tsx to reduce file size.
 * Also used by SharedDashboard.tsx.
 */

import { useState, useCallback, useMemo, memo } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'
import { createStableKey as createStableCacheKey } from '@/lib/stable-key'
import { useTouchHover } from '@/hooks/useMobile'
import {
  Settings2,
  Copy,
  Trash2,
} from 'lucide-react'
import { Button } from '@/components/ui/button'
import type { DashboardComponent, DataSourceOrList, DataSource } from '@/types/dashboard'
import { getSourceId, resolveDataSource } from '@/types/dashboard'
import { useStore } from '@/store'
import { findDevice } from '@/lib/deviceUtils'
import ComponentRenderer, { ComponentErrorFallback } from '@/components/dashboard/registry/ComponentRenderer'
import { ErrorBoundary } from '@/components/shared/ErrorBoundary'
import { getComponentMeta } from '@/components/dashboard/registry/registry'
import { useDeviceBindingStatus } from '@/components/dashboard/shared/useDeviceBindingStatus'
import { DanglingBindingState, StaleDataBadge } from '@/components/dashboard/shared/BindingStateOverlays'

// Direct imports for built-in components (bypass ComponentRenderer to avoid
// its store subscriptions causing blank frames during scroll)
import { ValueCard } from '@/components/dashboard/generic/ValueCard'
import { LEDIndicator } from '@/components/dashboard/generic/LEDIndicator'
import { Sparkline } from '@/components/dashboard/generic/Sparkline'
import { ProgressBar } from '@/components/dashboard/generic/ProgressBar'
import { LineChart } from '@/components/dashboard/generic/LineChart'
import { AreaChart } from '@/components/dashboard/generic/LineChart'
import { BarChart } from '@/components/dashboard/generic/BarChart'
import { PieChart } from '@/components/dashboard/generic/PieChart'
import { confirm } from '@/components/ui/use-confirm'
import { CommandButton } from '@/components/dashboard/generic/CommandButton'
import { ImageDisplay } from '@/components/dashboard/generic/ImageDisplay'
import { ImageHistory } from '@/components/dashboard/generic/ImageHistory'
import { WebDisplay } from '@/components/dashboard/generic/WebDisplay'
import { MarkdownDisplay } from '@/components/dashboard/generic/MarkdownDisplay'
import { MapDisplay } from '@/components/dashboard/generic/MapDisplay'
import { VideoDisplay } from '@/components/dashboard/generic/VideoDisplay'
import { CustomLayer } from '@/components/dashboard/generic/CustomLayer'

const builtInTypes = new Set([
  'value-card', 'counter', 'metric-card',
  'led-indicator', 'sparkline', 'progress-bar',
  'line-chart', 'area-chart', 'bar-chart', 'pie-chart',
  'toggle-switch', 'image-display', 'image-history',
  'web-display', 'markdown-display', 'map-display', 'video-display', 'custom-layer',
])

const builtInComponentMap: Record<string, React.ComponentType<any>> = {
  'value-card': ValueCard,
  'counter': ValueCard,
  'metric-card': ValueCard,
  'led-indicator': LEDIndicator,
  'sparkline': Sparkline,
  'progress-bar': ProgressBar,
  'line-chart': LineChart,
  'area-chart': AreaChart,
  'bar-chart': BarChart,
  'pie-chart': PieChart,
  'toggle-switch': CommandButton,
  'image-display': ImageDisplay,
  'image-history': ImageHistory,
  'web-display': WebDisplay,
  'markdown-display': MarkdownDisplay,
  'map-display': MapDisplay,
  'video-display': VideoDisplay,
  'custom-layer': CustomLayer,
}

// ============================================================================
// BuiltInComponent — lightweight direct renderer for built-in widgets
// ============================================================================

const BuiltInComponent = memo(function BuiltInComponent({
  component,
  config,
  dataSource,
  display,
  editMode,
  className,
}: {
  component: DashboardComponent
  config: Record<string, any>
  dataSource: any
  display: Record<string, any>
  editMode?: boolean
  className?: string
}) {
  const Comp = builtInComponentMap[component.type]
  const bindingStatus = useDeviceBindingStatus(dataSource)
  const devices = useStore((s) => s.devices)

  // Cards created from the library are pre-titled with the entry's display
  // name ("Value Card") — treat that as unnamed so single device bindings
  // render a meaningful "Field · Device name" label instead of the type.
  const resolvedTitle = useMemo(() => {
    const componentName = getComponentMeta(component.type)?.name?.trim()
    const isLibraryDefault = (t?: string) =>
      !!t && !!componentName && t.trim().toLowerCase() === componentName.toLowerCase()
    const explicit = [component.title, config.title].find(
      (x): x is string => typeof x === 'string' && !!x.trim() && !isLibraryDefault(x),
    )
    if (explicit) return explicit

    const sources = Array.isArray(dataSource) ? dataSource : (dataSource ? [dataSource] : [])
    let auto = ''
    if (sources.length === 1) {
      const ds = resolveDataSource(sources[0])
      const field = typeof ds.field === 'string' && ds.field ? ds.field : ''
      const prettyField = field.replace(/[_-]+/g, ' ').replace(/\b\w/g, c => c.toUpperCase())
      const deviceName = ds.source === 'device' && typeof ds.id === 'string'
        ? findDevice(devices ?? [], ds.id)?.name
        : undefined
      auto = [prettyField, deviceName].filter(Boolean).join(' · ')
    }
    return auto || componentName || undefined
  }, [dataSource, component.title, config.title, component.type, devices])

  if (!Comp) return null

  // Every bound device was removed from the registry — the widget can never
  // receive data again. Say so instead of a misleading "No Data Available".
  if (bindingStatus.allDangling) {
    return (
      <DanglingBindingState
        deviceIds={bindingStatus.danglingDeviceIds}
        className={className}
      />
    )
  }

  const { editMode: _em, transform: _t, ...restConfig } = config
  const { transform: _dt, ...restDisplay } = display

  const widget = (
    <Comp
      dataSource={dataSource}
      editMode={editMode}
      {...restConfig}
      {...restDisplay}
      title={resolvedTitle}
      className={className}
    />
  )

  // Bound device exists but is offline/idle — the card keeps showing the last
  // reported value. Badge it so it can't be mistaken for a live reading.
  // Hidden in edit mode (the hover action toolbar occupies the same corner).
  if (bindingStatus.staleDeviceIds.length > 0 && !editMode) {
    return (
      <div className="relative w-full h-full">
        {widget}
        <StaleDataBadge
          deviceIds={bindingStatus.staleDeviceIds}
          className="absolute top-1.5 right-1.5 z-10"
        />
      </div>
    )
  }

  return widget
})

// ============================================================================
// Telemetry cache + helpers
// ============================================================================

// Use Map for atomic operations (safe under React 18 concurrent rendering)
const telemetryCache = new Map<string, { data: any; ts: number }>()
const MAX_CACHE_SIZE = 100
const CACHE_TTL = 5 * 60 * 1000 // 5 minutes

/** Clear the Renderers telemetry cache — call on dashboard switch */
export function clearRenderersTelemetryCache(): void {
  telemetryCache.clear()
}

export function scheduleDashboardIdleTask(task: () => void, timeout = 1500): () => void {
  if (typeof window === 'undefined') { task(); return () => {} }
  const requestIdle = (window as any).requestIdleCallback as
    | ((cb: () => void, options?: { timeout: number }) => number)
    | undefined
  const cancelIdle = (window as any).cancelIdleCallback as ((id: number) => void) | undefined
  if (requestIdle && cancelIdle) {
    const id = requestIdle(task, { timeout })
    return () => cancelIdle(id)
  }
  const timer = window.setTimeout(task, Math.min(timeout, 300))
  return () => window.clearTimeout(timer)
}

function getTelemetryDataSource(dataSource: DataSourceOrList | undefined): DataSourceOrList | undefined {
  if (!dataSource) return undefined
  // Sort array data sources by a stable key to avoid cache misses from different orderings
  const sortedSource = Array.isArray(dataSource)
    ? [...dataSource].sort((a, b) => {
        const keyA = `${a.type}:${a.sourceId || a.extensionId || ''}:${a.metricId || a.property || ''}`
        const keyB = `${b.type}:${b.sourceId || b.extensionId || ''}:${b.metricId || b.property || ''}`
        return keyA.localeCompare(keyB)
      })
    : dataSource
  const cacheKey = createStableCacheKey(sortedSource)
  const cached = telemetryCache.get(cacheKey)
  if (cached && Date.now() - cached.ts < CACHE_TTL) {
    return cached.data
  } else if (cached) {
    // Expired — evict
    telemetryCache.delete(cacheKey)
  }
  const normalizeAndConvert = (ds: DataSource): DataSource => {
    if (ds.type === 'telemetry') return ds
    if (ds.type === 'device' && getSourceId(ds) && ds.property) {
      const agg = ds.aggregateExt ?? 'raw'
      return {
        ...ds,
        type: 'telemetry' as const,
        metricId: ds.property,
        aggregateExt: agg,
        limit: ds.limit ?? 50,
        timeRange: ds.timeRange ?? 1,
      }
    }
    return ds
  }
  let result: DataSourceOrList
  if (Array.isArray(sortedSource)) {
    result = sortedSource.map(normalizeAndConvert)
  } else {
    result = normalizeAndConvert(sortedSource)
  }
  telemetryCache.set(cacheKey, { data: result, ts: Date.now() })
  if (telemetryCache.size > MAX_CACHE_SIZE) {
    // Evict oldest entry (first key in insertion order)
    const oldest = telemetryCache.keys().next().value
    if (oldest) telemetryCache.delete(oldest)
  }
  return result
}

// ============================================================================
// Common display helpers
// ============================================================================

export function getCommonDisplayProps(component: DashboardComponent) {
  const config = (component as any).config || {}
  const w = component.position.w
  const h = component.position.h
  const area = w * h
  let calculatedSize: 'xs' | 'sm' | 'md' | 'lg' = 'md'
  if (area <= 3) calculatedSize = 'xs'
  else if (area <= 6) calculatedSize = 'sm'
  else if (area <= 12) calculatedSize = 'md'
  else calculatedSize = 'lg'
  return {
    size: config.size || calculatedSize,
    showCard: config.showCard ?? true,
    title: component.title || config.title,
    className: 'w-full h-full',
    color: config.color,
  }
}

function getSpreadableProps(componentType: string, commonProps: Record<string, unknown>): Record<string, unknown> {
  const noStandardSize = ['led-indicator', 'toggle-switch', 'heading', 'tabs', 'agent-monitor-widget', 'ai-analyst']
  const noShowCard = ['value-card', 'led-indicator', 'sparkline', 'progress-bar', 'toggle-switch', 'heading', 'alert-banner', 'agent-monitor-widget', 'ai-analyst', 'tabs']
  const noTitle = ['sparkline', 'led-indicator', 'progress-bar', 'toggle-switch', 'heading', 'alert-banner', 'tabs', 'agent-monitor-widget']
  const result: Record<string, unknown> = {}
  if (!noStandardSize.includes(componentType)) result.size = commonProps.size
  if (!noShowCard.includes(componentType)) result.showCard = commonProps.showCard
  if (!noTitle.includes(componentType)) result.title = commonProps.title
  result.className = commonProps.className
  if (commonProps.color) result.color = commonProps.color
  return result
}

export function getChartHeight(component: DashboardComponent): number | 'auto' {
  const h = component.position.h
  return Math.max(h * 120 - 60, 120)
}

// ============================================================================
// renderDashboardComponent — main widget renderer
// ============================================================================

export function renderDashboardComponent(
  component: DashboardComponent,
  editMode?: boolean,
  onDataSourceChange?: (dataSource: Record<string, any>) => void,
  onConfigChange?: (config: Record<string, any>) => void,
  openFullscreen?: (content: React.ReactNode) => void,
  closeFullscreen?: () => void,
) {
  const config = (component as any).config || {}
  const dataSource = (component as any).dataSource
  const display = (component as any).display || {}

  if (!builtInTypes.has(component.type)) {
    const normalizedComponent = {
      ...component,
      config: { ...config, editMode, height: config.height ?? getChartHeight(component) },
    } as DashboardComponent
    return (
      <ComponentRenderer
        component={normalizedComponent}
        className="w-full h-full"
        onDataSourceChange={onDataSourceChange}
        onConfigChange={onConfigChange}
        openFullscreen={openFullscreen}
        closeFullscreen={closeFullscreen}
      />
    )
  }

  // Per-card boundary for built-in widgets. Without it a single throwing card
  // bubbles to the route-level boundary and replaces the whole dashboard page.
  // This is the SYNCHRONOUS render path (direct component, no async loading),
  // so it does not hit the StrictMode double-mount + async-ErrorBoundary race
  // that CLAUDE.md gotcha #11 warns about for ComponentRenderer's load branch.
  return (
    <ErrorBoundary
      resetKey={`${component.id}:${component.type}`}
      fallback={<ComponentErrorFallback className="w-full h-full" />}
    >
      <BuiltInComponent
        component={component}
        config={config}
        dataSource={dataSource}
        display={display}
        editMode={editMode}
        className="w-full h-full"
      />
    </ErrorBoundary>
  )
}

// ============================================================================
// ComponentWrapper — edit-mode overlay wrapper
// ============================================================================

interface ComponentWrapperProps {
  component: DashboardComponent
  children: React.ReactNode
  editMode: boolean
  onOpenConfig: (componentId: string) => void
  onRemove: (componentId: string) => void
  onDuplicate: (componentId: string) => void
  onSelect?: (component: DashboardComponent | null) => void
  selectedComponentId?: string | null
  isMobile?: boolean
}

const ComponentWrapper = memo(function ComponentWrapper({
  component,
  children,
  editMode,
  onOpenConfig,
  onRemove,
  onDuplicate,
  onSelect,
  selectedComponentId,
  isMobile = false,
}: ComponentWrapperProps) {
  const [isHovered, setIsHovered] = useState(false)
  const { t } = useTranslation('dashboardComponents')
  const { isHovered: isTouchHovered, hoverProps } = useTouchHover({ enabled: editMode && !isMobile })

  const handleMouseEnter = useCallback(() => setIsHovered(true), [])
  const handleMouseLeave = useCallback(() => setIsHovered(false), [])
  const handleConfigClick = useCallback(() => onOpenConfig(component.id), [component.id, onOpenConfig])
  const handleDuplicateClick = useCallback(() => onDuplicate(component.id), [component.id, onDuplicate])

  const handleEditButtonClick = useCallback(() => {
    if (isMobile && editMode && onSelect) onSelect(component)
  }, [isMobile, editMode, onSelect, component])
  const handleEditButtonMouseEvent = useCallback((e: React.MouseEvent) => { e.stopPropagation(); handleEditButtonClick() }, [handleEditButtonClick])
  const handleEditButtonTouchEvent = useCallback((e: React.TouchEvent) => { e.preventDefault(); e.stopPropagation(); handleEditButtonClick() }, [handleEditButtonClick])

  const shouldShowActions = editMode && (isHovered || isTouchHovered) && !isMobile
  const isSelected = selectedComponentId === component.id

  return (
    <div className={cn('relative h-full')} {...(!isMobile ? hoverProps : {})}>
      <div className="h-full w-full flex flex-col">{children}</div>
      {shouldShowActions && (
        <div className="absolute top-2 right-2 z-10 flex gap-1">
          <Button variant="secondary" size="icon" className="bg-bg-90 backdrop-blur" onClick={handleConfigClick}>
            <Settings2 className="h-4 w-4" />
          </Button>
          <Button variant="secondary" size="icon" className="bg-bg-90 backdrop-blur" onClick={handleDuplicateClick}>
            <Copy className="h-4 w-4" />
          </Button>
          <Button variant="secondary" size="icon" className="bg-bg-90 backdrop-blur hover:bg-destructive hover:text-destructive-foreground transition-colors"
            onClick={() => { confirm({ title: t('componentWrapper.remove'), description: t('componentWrapper.removeConfirm'), confirmText: t('componentWrapper.remove'), variant: 'destructive' }).then(ok => { if (ok) onRemove(component.id) }) }}>
            <Trash2 className="h-4 w-4" />
          </Button>
        </div>
      )}
      {isMobile && editMode && (
        <div className={cn('absolute inset-0 z-10 flex items-center justify-center rounded-lg', isSelected ? 'bg-transparent' : 'bg-muted-30')}>
          {onSelect && (
            <button className={cn('absolute inset-0', isSelected ? 'pointer-events-none' : 'cursor-pointer')}
              onClick={handleEditButtonMouseEvent} onTouchEnd={handleEditButtonTouchEvent} aria-label="Select component" />
          )}
          {isSelected && (
            <div className="absolute bottom-2 right-2 z-20 flex gap-1">
              <Button variant="secondary" size="xs" onClick={handleConfigClick}>
                <Settings2 className="h-3 w-3 mr-1" />{t('componentWrapper.config')}
              </Button>
              <Button variant="secondary" size="xs" onClick={handleDuplicateClick}>
                <Copy className="h-3 w-3 mr-1" />{t('componentWrapper.copy')}
              </Button>
              <Button variant="secondary" size="xs" className="hover:bg-destructive hover:text-destructive-foreground"
                onClick={() => { confirm({ title: t('componentWrapper.remove'), description: t('componentWrapper.removeConfirm'), confirmText: t('componentWrapper.remove'), variant: 'destructive' }).then(ok => { if (ok) onRemove(component.id) }) }}>
                <Trash2 className="h-3 w-3 mr-1" />{t('componentWrapper.remove')}
              </Button>
            </div>
          )}
        </div>
      )}
    </div>
  )
})

export { ComponentWrapper }
export { builtInTypes, builtInComponentMap }
