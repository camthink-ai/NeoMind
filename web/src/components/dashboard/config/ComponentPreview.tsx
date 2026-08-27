/**
 * Component Preview
 *
 * Shows a live preview of a dashboard component with real-time data.
 * Used in the configuration dialog to visualize changes as they are made.
 * Auto-scales content proportionally to fit within container.
 */

import { memo, useRef, useEffect, useState, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { Skeleton } from '@/components/ui/skeleton'
import { Eye, AlertCircle, Loader2 } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import ComponentRenderer from '@/components/dashboard/registry/ComponentRenderer'
import { getComponentMeta } from '@/components/dashboard/registry/registry'
import type { DashboardComponent, DataSource, ImplementedComponentType } from '@/types/dashboard'
import { getSourceId } from '@/types/dashboard'
import { createStableKey } from '@/lib/stable-key'
import { responsiveCols } from '@/design-system/tokens/size'

/**
 * Create a simple key to detect dataSource changes
 * Includes transform fields (timeWindow, aggregateExt, limit) so that
 * config changes in the Transform tab trigger a preview re-render.
 */
function createDataSourceKey(ds: DataSource | undefined): string {
  if (!ds) return 'no-ds'
  const tw = ds.timeWindow
    ? `${ds.timeWindow.type}:${ds.timeWindow.startTime ?? ''}:${ds.timeWindow.endTime ?? ''}`
    : ''
  return `${ds.type}:${getSourceId(ds) || ''}:${ds.metricId || ds.property || ds.infoProperty || ''}:${ds.command || ''}:${ds.aggregateExt ?? ''}:${tw}:${ds.limit ?? ''}:${ds.timeRange ?? ''}`
}

export interface ComponentPreviewProps {
  componentType: string
  config: Record<string, unknown>
  dataSource?: DataSource
  title?: string
  showHeader?: boolean
  className?: string
  /** Maximum height for the preview content area (default: 200). Ignored when fillContainer is true. */
  maxContentHeight?: number
  /** When true, the preview content area fills the available container height instead of using maxContentHeight */
  fillContainer?: boolean
  /** Override grid width (in grid units) for aspect ratio calculation */
  gridW?: number
  /** Override grid height (in grid units) for aspect ratio calculation */
  gridH?: number
}

// Match DashboardGrid's exact rendering configuration
const GRID_ROW_HEIGHT = 60
const GRID_MARGIN: [number, number] = [4, 4]
const GRID_CONTAINER_PADDING: [number, number] = [4, 4]
const GRID_COLS = responsiveCols.lg  // 12

// Variant-aware height overrides for components whose variants need different sizes
function getVariantAwareHeight(componentType: string, config: Record<string, unknown>, baseH: number): number {
  if (componentType === 'progress-bar') {
    const variant = config.variant as string
    if (variant === 'icon' || variant === 'circular') return Math.max(baseH, 2)
  }
  return baseH
}

export const ComponentPreview = memo(function ComponentPreview({
  componentType,
  config,
  dataSource,
  title,
  showHeader = true,
  className,
  maxContentHeight = 200,
  fillContainer = false,
  gridW: overrideGridW,
  gridH: overrideGridH,
}: ComponentPreviewProps) {
  const { t } = useTranslation('dashboardComponents')
  const meta = getComponentMeta(componentType as ImplementedComponentType)

  // Track data source changes to show transition
  const [prevDataSourceKey, setPrevDataSourceKey] = useState<string>(() => createDataSourceKey(dataSource))
  const [isTransitioning, setIsTransitioning] = useState(false)

  // Track config changes for components with static content
  const [prevConfigKey, setPrevConfigKey] = useState<string>(() => createStableKey(config))

  // Use ref to track the active timer for cleanup
  const transitionTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  // Refs for auto-scaling — we use a simpler approach: render the component
  // at the container's actual size with correct aspect ratio, no CSS transform scale.
  // This is critical for chart components that use ResponsiveContainer, since
  // transform:scale doesn't change getBoundingClientRect() measurements.
  const containerRef = useRef<HTMLDivElement>(null)
  const [containerSize, setContainerSize] = useState<{ width: number; height: number } | null>(null)

  // Detect dataSource changes
  useEffect(() => {
    const newKey = createDataSourceKey(dataSource)
    if (newKey !== prevDataSourceKey) {
      if (transitionTimerRef.current) {
        clearTimeout(transitionTimerRef.current)
      }

      setIsTransitioning(true)

      transitionTimerRef.current = setTimeout(() => {
        setIsTransitioning(false)
        transitionTimerRef.current = null
      }, 200)

      setPrevDataSourceKey(newKey)
    }
  }, [dataSource, prevDataSourceKey])

  // Detect config changes
  useEffect(() => {
    const newKey = createStableKey(config)
    if (newKey !== prevConfigKey) {
      if (transitionTimerRef.current) {
        clearTimeout(transitionTimerRef.current)
      }

      setIsTransitioning(true)

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
    enabled: !!dataSource && meta?.hasDataSource,
  })

  // Update prevDataRef when we successfully get data
  useEffect(() => {
    if (!loading && !error) {
      if (data !== null && data !== undefined) {
        prevDataRef.current = data
      }
      hasLoadedOnceRef.current = true
    }
  }, [data, loading, error])

  // Use component's size from props or registry defaults (with variant-aware height)
  const baseW = overrideGridW ?? meta?.sizeConstraints.defaultW ?? 4
  const baseH = overrideGridH ?? meta?.sizeConstraints.defaultH ?? 3
  const defaultH = getVariantAwareHeight(componentType, config, baseH)
  const defaultW = baseW

  // Build a mock component for rendering with actual size
  const componentDisplayTitle = title || (config.label as string) || (config.title as string) || ''

  const mockComponent: DashboardComponent = {
    id: 'preview',
    type: componentType as ImplementedComponentType,
    position: { x: 0, y: 0, w: defaultW, h: defaultH },
    title: componentDisplayTitle,
    config: {
      ...config,
      editMode: true,
    },
    dataSource,
  }

  const updateContainerSize = useCallback(() => {
    if (!containerRef.current) return

    const containerWidth = containerRef.current.clientWidth
    const containerHeight = containerRef.current.clientHeight

    if (containerWidth <= 0 || containerHeight <= 0) return

    // Use the actual dashboard width (viewport width) as reference,
    // not the preview container width — the preview is narrow but the
    // dashboard is wide, so using the preview width would give wrong proportions.
    const dashboardWidth = typeof window !== 'undefined' ? window.innerWidth : 1200

    // Match react-grid-layout's exact pixel calculation:
    //   colWidth = (containerWidth - margin*(cols-1) - padding*2) / cols
    //   itemWidth = colWidth * w + (w-1) * margin
    //   itemHeight = rowHeight * h + (h-1) * margin
    const colWidth = (dashboardWidth - GRID_MARGIN[0] * (GRID_COLS - 1) - GRID_CONTAINER_PADDING[0] * 2) / GRID_COLS
    const pixelW = colWidth * defaultW + Math.max(0, defaultW - 1) * GRID_MARGIN[0]
    const pixelH = GRID_ROW_HEIGHT * defaultH + Math.max(0, defaultH - 1) * GRID_MARGIN[1]
    const aspectRatio = pixelW / pixelH

    // Fit component proportion into preview container
    const fitWidth = Math.min(containerWidth, containerHeight * aspectRatio)
    const fitHeight = fitWidth / aspectRatio

    setContainerSize({ width: Math.round(fitWidth), height: Math.round(fitHeight) })
  }, [defaultW, defaultH])

  // Update size on mount and when container/props change
  useEffect(() => {
    const timer = requestAnimationFrame(() => {
      updateContainerSize()
    })

    const resizeObserver = new ResizeObserver(() => {
      updateContainerSize()
    })

    if (containerRef.current) {
      resizeObserver.observe(containerRef.current)
    }

    return () => {
      cancelAnimationFrame(timer)
      resizeObserver.disconnect()
    }
  }, [updateContainerSize])

  // Show previous data during loading (except on first load)
  const showLoading = loading && !hasLoadedOnceRef.current
  const hasError = !!error

  return (
    <div className={cn('flex flex-col overflow-hidden', fillContainer && 'h-full', className)}>
      {/* Header */}
      {showHeader && (
        <div className="flex items-center justify-between px-3 py-2 border-b bg-muted-30 shrink-0">
          <div className="flex items-center gap-2">
            <Eye className="h-4 w-4 text-muted-foreground" />
            <span className="text-sm font-medium">{t('componentPreview.title')}</span>
            {(loading || isTransitioning) && (
              <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
            )}
          </div>
        </div>
      )}

      {/* Preview area — outer container fills space, inner is proportionally sized */}
      <div
        ref={containerRef}
        className={cn(
          'w-full bg-muted overflow-hidden relative',
          'flex items-center justify-center',
          'transition-opacity duration-normal',
          isTransitioning && 'opacity-60',
          fillContainer ? 'flex-1 min-h-0' : ''
        )}
        style={!fillContainer ? { height: `${maxContentHeight}px` } : undefined}
      >
        {showLoading ? (
          <div className="w-full h-full flex items-center justify-center">
            <Skeleton className="w-full h-full" />
          </div>
        ) : hasError ? (
          <div className="w-full h-full flex flex-col items-center justify-center text-muted-foreground p-4 text-center">
            <AlertCircle className="h-8 w-8 text-error mb-2" />
            <p className="text-sm">{t('componentPreview.loadingFailed')}</p>
            <p className="text-xs text-muted-foreground mt-1">{t('componentPreview.usingStaticData')}</p>
          </div>
        ) : (
          <div
            className={cn(
              'transition-all duration-normal ease-out',
              isTransitioning && 'blur-[1px]'
            )}
            style={containerSize ? {
              width: containerSize.width,
              height: containerSize.height,
            } : undefined}
          >
            <ComponentRenderer
              key={`preview-${componentType}-${(config.backgroundType as string) || 'default'}`}
              component={mockComponent}
            />
          </div>
        )}
      </div>

      {/* Footer with component info */}
      <div className="px-3 py-1.5 border-t shrink-0">
        <div className="flex items-center justify-between text-xs text-muted-foreground">
          <span>{meta?.name || componentType}</span>
          <div className="flex items-center gap-2">
            {containerSize && (
              <span className="text-muted-foreground tabular-nums">
                {containerSize.width}×{containerSize.height}
              </span>
            )}
            <span className="text-muted-foreground">
              {defaultW}×{defaultH}
            </span>
          </div>
        </div>
      </div>
    </div>
  )
}, (prevProps, nextProps) => {
  const prevConfigKey = createStableKey(prevProps.config)
  const nextConfigKey = createStableKey(nextProps.config)
  const prevDsKey = createDataSourceKey(prevProps.dataSource)
  const nextDsKey = createDataSourceKey(nextProps.dataSource)

  return (
    prevProps.componentType === nextProps.componentType &&
    prevProps.title === nextProps.title &&
    prevConfigKey === nextConfigKey &&
    prevDsKey === nextDsKey &&
    prevProps.maxContentHeight === nextProps.maxContentHeight &&
    prevProps.fillContainer === nextProps.fillContainer &&
    prevProps.gridW === nextProps.gridW &&
    prevProps.gridH === nextProps.gridH
  )
})
