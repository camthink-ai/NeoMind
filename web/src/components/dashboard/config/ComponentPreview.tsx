/**
 * Component Preview
 *
 * Shows a live preview of a dashboard component with real-time data.
 * Used in the configuration dialog to visualize changes as they are made.
 * Uses the component's actual default size from the registry.
 */

import { memo, useRef, useEffect, useState } from 'react'
import { Skeleton } from '@/components/ui/skeleton'
import { Eye, EyeOff, AlertCircle, Loader2 } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import ComponentRenderer from '@/components/dashboard/registry/ComponentRenderer'
import { getComponentMeta } from '@/components/dashboard/registry/registry'
import type { DashboardComponent, DataSource, ImplementedComponentType } from '@/types/dashboard'

// Helper function to create stable key for comparison
function createStableKey(obj: any): string {
  if (obj === null || obj === undefined) return ''
  if (typeof obj !== 'object') return String(obj)
  if (Array.isArray(obj)) return '[' + obj.map(createStableKey).join(',') + ']'
  const sortedKeys = Object.keys(obj).sort()
  return '{' + sortedKeys.map(k => `"${k}":${createStableKey(obj[k])}`).join(',') + '}'
}

/**
 * Create a simple key to detect dataSource changes
 */
function createDataSourceKey(ds: DataSource | undefined): string {
  if (!ds) return 'no-ds'
  return `${ds.type}:${ds.deviceId || ''}:${ds.metricId || ds.property || ds.infoProperty || ''}:${ds.command || ''}`
}

export interface ComponentPreviewProps {
  componentType: string
  config: Record<string, unknown>
  dataSource?: DataSource
  title?: string
  showHeader?: boolean
  className?: string
}

// Grid cell height in pixels (typical dashboard grid)
const GRID_CELL_HEIGHT = 80

// Minimum preview height in pixels
const MIN_PREVIEW_HEIGHT = 140

/**
 * Format data source label for display
 */
function formatDataSourceLabel(ds: DataSource | undefined): string {
  if (!ds) return '无数据源'

  switch (ds.type) {
    case 'device':
      return `设备: ${ds.deviceId}${ds.property ? ` (${ds.property})` : ''}`
    case 'device-info':
      return `设备信息: ${ds.deviceId}${ds.infoProperty ? ` (${ds.infoProperty})` : ''}`
    case 'metric':
      return `指标: ${ds.metricId || '未指定'}`
    case 'command':
      return `指令: ${ds.deviceId} → ${ds.command || 'toggle'}`
    case 'telemetry':
      return `遥测: ${ds.deviceId} / ${ds.metricId || 'raw'}`
    case 'api':
      return `API: ${ds.endpoint || '自定义'}`
    case 'websocket':
      return `WebSocket: ${ds.endpoint || '实时'}`
    case 'static':
      return `静态: ${JSON.stringify(ds.staticValue)?.slice(0, 20) || '值'}`
    default:
      return '未知类型'
  }
}

export const ComponentPreview = memo(function ComponentPreview({
  componentType,
  config,
  dataSource,
  title,
  showHeader = true,
  className,
}: ComponentPreviewProps) {
  const meta = getComponentMeta(componentType as ImplementedComponentType)

  // Debug logging for custom-layer
  if (componentType === 'custom-layer') {
    console.log('[ComponentPreview] config.backgroundType:', config.backgroundType)
  }

  // Track data source changes to show transition
  const [prevDataSourceKey, setPrevDataSourceKey] = useState<string>(() => createDataSourceKey(dataSource))
  const [isTransitioning, setIsTransitioning] = useState(false)

  // Track config changes for components with static content (like MarkdownDisplay)
  const [prevConfigKey, setPrevConfigKey] = useState<string>(() => createStableKey(config))

  // Use ref to track the active timer for cleanup
  const transitionTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  // Detect dataSource changes
  useEffect(() => {
    const newKey = createDataSourceKey(dataSource)
    if (newKey !== prevDataSourceKey) {
      // Clear any existing timer
      if (transitionTimerRef.current) {
        clearTimeout(transitionTimerRef.current)
      }

      // Show transition state
      setIsTransitioning(true)
      
      // Set new timer to hide transition
      transitionTimerRef.current = setTimeout(() => {
        setIsTransitioning(false)
        transitionTimerRef.current = null
      }, 200)
      
      setPrevDataSourceKey(newKey)
    }
  }, [dataSource, prevDataSourceKey])

  // Detect config changes (for static content components like MarkdownDisplay)
  useEffect(() => {
    const newKey = createStableKey(config)
    if (newKey !== prevConfigKey) {
      // Clear any existing timer
      if (transitionTimerRef.current) {
        clearTimeout(transitionTimerRef.current)
      }

      // Force transition for config changes
      setIsTransitioning(true)
      
      // Set new timer to hide transition
      transitionTimerRef.current = setTimeout(() => {
        setIsTransitioning(false)
        transitionTimerRef.current = null
      }, 150)
      
      setPrevConfigKey(newKey)
    }
  }, [config, prevConfigKey])

  // Cleanup timer on unmount
  useEffect(() => {
    return () => {
      if (transitionTimerRef.current) {
        clearTimeout(transitionTimerRef.current)
      }
    }
  }, [])

  // Track previous data to show during loading (prevents flicker)
  const prevDataRef = useRef<any>(null)
  const hasLoadedOnceRef = useRef(false)

  // Try to fetch real data for preview
  const { data, loading, error } = useDataSource(dataSource, {
    // Only fetch if we have a valid data source
    enabled: !!dataSource && meta?.hasDataSource,
  })

  // Update prevDataRef when we successfully get data
  useEffect(() => {
    if (!loading && !error) {
      if (data !== null && data !== undefined) {
        prevDataRef.current = data
      }
      // Mark as loaded even if data is null (e.g., when no dataSource)
      // This prevents showing loading skeleton indefinitely
      hasLoadedOnceRef.current = true
    }
  }, [data, loading, error])

  // Use component's default size from registry
  const defaultW = meta?.sizeConstraints.defaultW ?? 4
  const defaultH = meta?.sizeConstraints.defaultH ?? 3

  // Build a mock component for rendering with actual default size
  const mockComponent: DashboardComponent = {
    id: 'preview',
    type: componentType as ImplementedComponentType,
    position: { x: 0, y: 0, w: defaultW, h: defaultH },
    title: title || config.title as string || '预览',
    config,
    dataSource,
  }

  // Calculate preview height based on component's default grid height
  // Limit max height to prevent overflow in config dialog
  const rawHeight = defaultH * GRID_CELL_HEIGHT + 32 // +32 for padding
  const maxPreviewHeight = 280 // Max height for preview area in dialog
  const previewHeight = Math.min(Math.max(MIN_PREVIEW_HEIGHT, rawHeight), maxPreviewHeight)

  // Show previous data during loading (except on first load)
  const displayData = loading && hasLoadedOnceRef.current ? prevDataRef.current : data
  const showLoading = loading && !hasLoadedOnceRef.current
  const hasData = !loading && !error && !!dataSource
  const hasError = !!error

  return (
    <div className={cn('flex flex-col overflow-hidden', className)}>
      {/* Header */}
      {showHeader && (
        <div className="flex items-center justify-between px-3 py-2 border-b bg-muted/30 shrink-0">
          <div className="flex items-center gap-2">
            <Eye className="h-4 w-4 text-muted-foreground" />
            <span className="text-sm font-medium">预览</span>
            {/* Loading indicator */}
            {(loading || isTransitioning) && (
              <Loader2 className="h-3 w-3 animate-spin text-muted-foreground/50" />
            )}
          </div>
        </div>
      )}

      {/* Preview area - dynamic height based on component size */}
      <div
        className={cn(
          'min-h-0 p-3 bg-muted/10 overflow-hidden relative',
          'transition-opacity duration-200',
          isTransitioning && 'opacity-60'
        )}
        style={{ height: `${previewHeight}px` }}
      >
        {showLoading ? (
          <div className="w-full h-full flex items-center justify-center">
            <Skeleton className="w-full h-full" />
          </div>
        ) : hasError ? (
          <div className="w-full h-full flex flex-col items-center justify-center text-muted-foreground p-4 text-center">
            <AlertCircle className="h-8 w-8 text-destructive/60 mb-2" />
            <p className="text-sm">预览数据加载失败</p>
            <p className="text-xs text-muted-foreground/60 mt-1">使用静态数据预览</p>
          </div>
        ) : (
          <div className={cn(
            'w-full h-full p-2 transition-all duration-200 ease-out overflow-hidden',
            isTransitioning && 'scale-[0.98] blur-[1px]'
          )}>
            <ComponentRenderer component={mockComponent} />
          </div>
        )}
      </div>

      {/* Footer with component info */}
      <div className="px-3 py-2 border-t bg-muted/20 shrink-0">
        <div className="flex items-center justify-between text-xs text-muted-foreground">
          <span>{meta?.name || componentType}</span>
          <span className="text-muted-foreground/60">
            {defaultW}×{defaultH}
          </span>
        </div>
      </div>
    </div>
  )
}, (prevProps, nextProps) => {
  // Simplified memo comparison - only skip re-render if everything is exactly the same
  return (
    prevProps.componentType === nextProps.componentType &&
    prevProps.title === nextProps.title &&
    prevProps.config === nextProps.config &&
    prevProps.dataSource === nextProps.dataSource
  )
})
