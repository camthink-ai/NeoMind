/**
 * Sparkline Component (Unified Styles)
 *
 * A shadcn/ui compliant mini chart for displaying trends.
 * Supports data binding and real-time updates.
 * Fully responsive and adaptive with comprehensive error handling.
 */

import { useRef, useMemo, memo } from 'react'
import { cn } from '@/lib/utils'
import { toNumberArray } from '@/design-system/utils/format'
import { dashboardComponentSize, dashboardCardBase } from '@/design-system/tokens/size'
import { Skeleton } from '@/components/ui/skeleton'
import {
  indicatorFontWeight,
  indicatorColors,
  getValueStateColor,
} from '@/design-system/tokens/indicator'

import type { DataSourceOrList, TelemetryAggregate, TimeWindowType } from '@/types/dashboard'
import { EmptyState, ErrorState, useChartPipeline } from '../shared'
import type { SingleValueMappingConfig } from '@/lib/dataMapping'
import { timeWindowToHours } from '@/lib/telemetryTransform'

// Static style constants to avoid re-creation on each render
const SVG_OVERFLOW_VISIBLE: React.CSSProperties = { overflow: 'visible' }

export interface SparklineProps {
  // Data source configuration
  dataSource?: DataSourceOrList

  // Data
  data?: number[] // Used if no dataSource

  // Display options
  width?: number
  height?: number
  responsive?: boolean

  // Card wrapper for dashboard use
  showCard?: boolean

  // Styling
  color?: string
  colorMode?: 'primary' | 'fixed' | 'value'  // value: like ProgressBar (based on latest value/max ratio)
  fill?: boolean
  strokeWidth?: number
  curved?: boolean

  // Threshold line
  showThreshold?: boolean
  threshold?: number
  thresholdColor?: string

  // Data mapping configuration
  dataMapping?: SingleValueMappingConfig

  // Value range for value-based coloring
  maxValue?: number  // For colorMode='value', determines color based on latestValue/maxValue ratio

  // Value display
  showValue?: boolean
  title?: string
  size?: 'sm' | 'md' | 'lg'

  // === Telemetry transform options ===
  // Time window for data
  timeWindow?: TimeWindowType
  // How to aggregate time-series data
  aggregate?: TelemetryAggregate

  // Edit mode (passed by config dialog preview)
  editMode?: boolean

  className?: string
}

// Default sample data for preview
const DEFAULT_SAMPLE_DATA = [12, 15, 13, 18, 14, 16, 19, 17, 20, 18, 22, 19, 21, 24, 22]

// Internal sparkline component that tracks container size
// Memoized to prevent re-renders when props haven't changed
const ResponsiveSparkline = memo(function ResponsiveSparkline({
  data: chartData,
  width: initialWidth,
  height,
  color,
  fill,
  strokeWidth,
  curved,
  showThreshold,
  threshold,
  thresholdColor,
  className,
}: {
  data: number[]
  width: number
  height: number
  color: string
  fill?: boolean
  strokeWidth?: number
  curved?: boolean
  showThreshold?: boolean
  threshold?: number
  thresholdColor?: string
  className?: string
}) {
  const containerRef = useRef<HTMLDivElement>(null)
  const gradientId = useRef(`sparkline-gradient-${Math.random().toString(36).substr(2, 9)}`).current

  // Use fixed viewBox with normalized coordinates (0-100 scale)
  // This prevents flickering when container resizes
  const VIEWBOX_WIDTH = 100
  const VIEWBOX_HEIGHT = 100

  // Memoize calculations to prevent unnecessary recalculations.
  // Hooks must run before the <2-points early return below — guard empty
  // input here instead of skipping the hook (rules-of-hooks).
  const { min, max, isFlatLine, range, points } = useMemo(() => {
    if (chartData.length === 0) {
      return { min: 0, max: 0, isFlatLine: true, range: 1, points: [] as { x: number; y: number; value: number }[] }
    }
    const min = chartData.reduce((a, b) => Math.min(a, b), Infinity)
    const max = chartData.reduce((a, b) => Math.max(a, b), -Infinity)
    const isFlatLine = max === min
    const range = max - min || 1

    // Calculate points using normalized 0-100 coordinates
    const points = chartData.map((v, i) => {
      const x = (i / (chartData.length - 1)) * VIEWBOX_WIDTH
      const y = isFlatLine
        ? VIEWBOX_HEIGHT / 2
        : VIEWBOX_HEIGHT - ((v - min) / range) * VIEWBOX_HEIGHT
      return { x, y, value: v }
    })

    return { min, max, isFlatLine, range, points }
  }, [chartData])

  // Memoize path string to prevent recalculation
  const pathD = useMemo(() => {
    if (curved && points.length > 2) {
      const curvePoints: string[] = []
      curvePoints.push(`M ${points[0].x} ${points[0].y}`)

      for (let i = 0; i < points.length - 1; i++) {
        const p0 = points[Math.max(0, i - 1)]
        const p1 = points[i]
        const p2 = points[i + 1]
        const p3 = points[Math.min(points.length - 1, i + 2)]

        const cp1x = p1.x + (p2.x - p0.x) / 6
        const cp1y = p1.y + (p2.y - p0.y) / 6
        const cp2x = p2.x - (p3.x - p1.x) / 6
        const cp2y = p2.y - (p3.y - p1.y) / 6

        curvePoints.push(`C ${cp1x} ${cp1y}, ${cp2x} ${cp2y}, ${p2.x} ${p2.y}`)
      }

      return curvePoints.join(' ')
    } else {
      return points.map((p, i) => `${i === 0 ? 'M' : 'L'} ${p.x} ${p.y}`).join(' ')
    }
  }, [points, curved])

  const fillPath = useMemo(() => {
    return `${pathD} L ${VIEWBOX_WIDTH} ${VIEWBOX_HEIGHT} L 0 ${VIEWBOX_HEIGHT} Z`
  }, [pathD])

  const thresholdY = useMemo(() => {
    if (showThreshold && threshold !== undefined) {
      // Clamp threshold to data range for proper positioning
      const clampedThreshold = Math.max(min, Math.min(max, threshold))
      if (isFlatLine) {
        return VIEWBOX_HEIGHT / 2
      }
      return VIEWBOX_HEIGHT - ((clampedThreshold - min) / range) * VIEWBOX_HEIGHT
    }
    return null
  }, [showThreshold, threshold, isFlatLine, min, max, range])

  // Guard: need at least 2 points to draw a line (after all hooks)
  if (chartData.length < 2) {
    return <div ref={containerRef} className={cn('w-full h-full relative', className)} />
  }

  return (
    <div ref={containerRef} className={cn('w-full h-full relative', className)}>
      <svg
        width="100%"
        height="100%"
        viewBox={`0 0 ${VIEWBOX_WIDTH} ${VIEWBOX_HEIGHT}`}
        preserveAspectRatio="none"
        style={SVG_OVERFLOW_VISIBLE}
      >
        <defs>
          <linearGradient id={gradientId} x1="0%" y1="0%" x2="0%" y2="100%">
            <stop offset="0%" stopColor={color} stopOpacity="0.3" />
            <stop offset="50%" stopColor={color} stopOpacity="0.1" />
            <stop offset="100%" stopColor={color} stopOpacity="0" />
          </linearGradient>

          <filter id={`glow-${gradientId}`} x="-50%" y="-50%" width="200%" height="200%">
            <feGaussianBlur stdDeviation="1.5" result="coloredBlur" />
            <feMerge>
              <feMergeNode in="coloredBlur" />
              <feMergeNode in="SourceGraphic" />
            </feMerge>
          </filter>
        </defs>

        {fill && (
          <path
            d={fillPath}
            fill={`url(#${gradientId})`}
          />
        )}

        <path
          d={pathD}
          fill="none"
          stroke={color}
          strokeWidth={strokeWidth}
          strokeLinecap="round"
          strokeLinejoin="round"
          vectorEffect="non-scaling-stroke"
          filter={`url(#glow-${gradientId})`}
        />

        {showThreshold && threshold !== undefined && thresholdY !== null && (
          <line
            x1={0}
            y1={thresholdY}
            x2={VIEWBOX_WIDTH}
            y2={thresholdY}
            stroke={thresholdColor}
            strokeWidth={0.75}
            strokeDasharray="3 3"
            vectorEffect="non-scaling-stroke"
            className="opacity-70"
          />
        )}
      </svg>

      {/* Last value indicator - rendered as HTML to avoid SVG distortion */}
      {points.length > 0 && (
        <div
          className="absolute pointer-events-none"
          style={{
            left: `${points[points.length - 1].x}%`,
            top: `${points[points.length - 1].y}%`,
            transform: 'translate(-50%, -50%)',
          }}
        >
          {/* Outer glow ring */}
          <div
            className="rounded-full"
            style={{
              width: '12px',
              height: '12px',
              backgroundColor: color,
              opacity: 0.2,
            }}
          />
          {/* Main dot */}
          <div
            className="absolute top-1/2 left-1/2 rounded-full -translate-x-1/2 -translate-y-1/2"
            style={{
              width: '7px',
              height: '7px',
              backgroundColor: color,
            }}
          />
        </div>
      )}
    </div>
  )
})

/**
 * Props for the extracted SparklineContent component
 */
interface SparklineContentProps {
  title?: string
  showValue?: boolean
  stats: { latestValue: number; dataMax: number; effectiveMax: number }
  sizeConfig: { labelText: string; valueText: string }
  chartHeight: number
  width: number
  chartData: number[]
  lineColor: string
  fill?: boolean
  strokeWidth?: number
  curved?: boolean
  showThreshold?: boolean
  effectiveThreshold?: number
  thresholdColor?: string
}

/**
 * Extracted top-level SparklineContent to prevent remount on each render.
 * Wrapped in React.memo so re-renders are skipped when props haven't changed.
 */
const SparklineContent = memo(function SparklineContent({
  title,
  showValue,
  stats,
  sizeConfig,
  chartHeight,
  width,
  chartData,
  lineColor,
  fill,
  strokeWidth,
  curved,
  showThreshold,
  effectiveThreshold,
  thresholdColor,
}: SparklineContentProps) {
  return (
    <>
      {/* Header with title and value */}
      {(title || showValue) && (
        <div className="flex items-center justify-between mb-2">
          {title && (
            <span className={cn(indicatorFontWeight.title, 'text-muted-foreground', sizeConfig.labelText)}>
              {title}
            </span>
          )}
          {showValue && (
            <span className={cn(indicatorFontWeight.value, 'text-foreground tabular-nums', sizeConfig.valueText)}>
              {stats.latestValue.toLocaleString(undefined, { maximumFractionDigits: 1 })}
            </span>
          )}
        </div>
      )}

      {/* Chart */}
      <div className={cn('flex-1 min-h-0', 'overflow-visible')} style={{ height: chartHeight }}>
        <ResponsiveSparkline
          data={chartData}
          width={width}
          height={chartHeight}
          color={lineColor}
          fill={fill}
          strokeWidth={strokeWidth}
          curved={curved}
          showThreshold={showThreshold}
          threshold={effectiveThreshold}
          thresholdColor={thresholdColor || indicatorColors.neutral.base}
        />
      </div>
    </>
  )
})


function SparklineComponent({
  dataSource,
  data: propData,
  width = 100,
  height,
  responsive = false,
  showCard = true,
  color,
  colorMode = 'fixed',
  fill = true,
  strokeWidth = 2,
  curved = true,
  showThreshold = false,
  threshold,
  thresholdColor,
  dataMapping,
  maxValue,
  showValue = true,
  title,
  size = 'md',
  timeWindow,
  aggregate = 'raw',
  editMode = false,
  className,
}: SparklineProps) {
  // Compute default time range from props (preserves 'last_24hours' default).
  // useChartPipeline's sourceTransform prefers each source's own timeWindow
  // when present, falling back to this value.
  const timeRange = useMemo(
    () => timeWindowToHours(timeWindow ?? 'last_24hours'),
    [timeWindow]
  )

  // Shared data pipeline — same as LineChart/AreaChart/BarChart/PieChart.
  // Replaces hand-written normalizeDataSource + telemetrySources conversion,
  // which double-normalized, keyed useDataSource on the transformed object
  // (causing resets), and collapsed min/max/sum aggregates to 'avg'.
  const { data, loading, error } = useChartPipeline<unknown>({
    dataSource,
    aggregate,
    limit: 50,
    timeRange,
    preserveMultiple: true,
  })

  // Prevent loading flash: only show skeleton when loading AND no data exists yet
  // Treat empty arrays as "no data" — the pipeline uses [] for empty fetches
  const hasData = data !== null && data !== undefined && !(Array.isArray(data) && data.length === 0)
  const showLoading = loading && !hasData

  // Check if dataSource is configured
  const hasDataSource = dataSource !== undefined

  // Convert data to number array — same pattern as LineChart/AreaChart
  const chartData = useMemo(() => {
    // In edit mode, always show data (sample if real data unavailable)
    if (error && !editMode) return []

    // When dataSource is configured, use live data only
    if (hasDataSource) {
      let rawData = data
      // Multi-source: flatten into single array
      if (Array.isArray(rawData) && rawData.length > 0 && Array.isArray(rawData[0])) {
        const allData: unknown[] = []
        for (const sourceData of rawData) {
          if (Array.isArray(sourceData)) allData.push(...sourceData)
        }
        rawData = allData.length > 0 ? allData : rawData
      }

      const result = toNumberArray(rawData, [])
      if (result.length >= 2) return result
      // Data source set but not enough data — show sample preview in editMode, empty otherwise
      if (editMode) return DEFAULT_SAMPLE_DATA
      return []
    }

    // No dataSource — use propData or sample data
    if (propData && Array.isArray(propData) && propData.length >= 2) {
      return propData
    }

    // Default sample data for preview mode
    return DEFAULT_SAMPLE_DATA
  }, [data, propData, error, hasDataSource, editMode])

  const sizeConfig = dashboardComponentSize[size]

  // Calculate chart height based on size
  const chartHeight = height ?? (size === 'sm' ? 40 : size === 'md' ? 60 : 80)

  // Memoize stats to prevent recalculation on every render
  const stats = useMemo(() => {
    const latestValue = chartData.length > 0 ? chartData[chartData.length - 1] : 0
    const dataMax = chartData.length > 0 ? chartData.reduce((a, b) => Math.max(a, b), -Infinity) : 0
    const effectiveMax = maxValue ?? dataMax ?? 100

    return {
      latestValue,
      dataMax,
      effectiveMax,
    }
  }, [chartData, maxValue])

  // Derive threshold from dataMapping if not explicitly provided
  const effectiveThreshold = threshold ?? dataMapping?.thresholds?.warning?.value

  // Memoize color calculation to prevent flickering
  const lineColor = useMemo(() => {
    if (colorMode === 'value') {
      return getValueStateColor(stats.latestValue, stats.effectiveMax)
    } else if (colorMode === 'primary') {
      return indicatorColors.primary.base
    }
    // 'fixed' mode - use the configured color or default to primary
    return color || indicatorColors.primary.base
  }, [color, colorMode, stats.latestValue, stats.effectiveMax])


  // Error state - use unified ErrorState (skip in editMode to keep preview visible)
  if (error && !editMode) {
    return <ErrorState size={size} className={className} />
  }

  // Loading state - show skeleton while fetching initial data
  // Keep showing loading as long as we have a dataSource and no data yet,
  // even if the initial fetch returned [] (retry/polling may still deliver data)
  if (loading && hasDataSource && chartData.length < 2) {
    return (
      <div className={cn(dashboardCardBase, 'h-full flex flex-col', sizeConfig.padding, className)}>
        <div className="flex items-center justify-between mb-2">
          {title && <Skeleton className="h-4 w-20" />}
          {showValue && <Skeleton className="h-5 w-12" />}
        </div>
        <Skeleton className="flex-1 w-full min-h-0" />
      </div>
    )
  }

  // Empty state - only show when loading is fully complete (not retrying/polling)
  if (!editMode && !loading && hasDataSource && chartData.length < 2) {
    return <EmptyState size={size} className={className} message={title ? `${title} - No Data Available` : undefined} />
  }

  // Card wrapper mode (default for dashboard use)
  if (showCard) {
    return (
      <div className={cn(dashboardCardBase, 'flex flex-col', sizeConfig.padding, className)}>
        <SparklineContent
          title={title}
          showValue={showValue}
          stats={stats}
          sizeConfig={sizeConfig}
          chartHeight={chartHeight}
          width={width}
          chartData={chartData}
          lineColor={lineColor}
          fill={fill}
          strokeWidth={strokeWidth}
          curved={curved}
          showThreshold={showThreshold}
          effectiveThreshold={effectiveThreshold}
          thresholdColor={thresholdColor}
        />
      </div>
    )
  }

  // Non-card mode (when used in custom layouts)
  return (
    <div className={cn('flex flex-col w-full', className)}>
      <SparklineContent
          title={title}
          showValue={showValue}
          stats={stats}
          sizeConfig={sizeConfig}
          chartHeight={chartHeight}
          width={width}
          chartData={chartData}
          lineColor={lineColor}
          fill={fill}
          strokeWidth={strokeWidth}
          curved={curved}
          showThreshold={showThreshold}
          effectiveThreshold={effectiveThreshold}
          thresholdColor={thresholdColor}
        />
    </div>
  )
}

export const Sparkline = memo(SparklineComponent)
