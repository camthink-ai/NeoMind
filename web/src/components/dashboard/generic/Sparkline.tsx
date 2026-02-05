/**
 * Sparkline Component (Unified Styles)
 *
 * A shadcn/ui compliant mini chart for displaying trends.
 * Supports data binding and real-time updates.
 * Fully responsive and adaptive with comprehensive error handling.
 */

import { useRef, useMemo, memo } from 'react'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { toNumberArray } from '@/design-system/utils/format'
import { dashboardComponentSize, dashboardCardBase } from '@/design-system/tokens/size'
import {
  indicatorFontWeight,
  indicatorColors,
  getValueStateColor,
} from '@/design-system/tokens/indicator'
import type { DataSourceOrList, TelemetryAggregate, TimeWindowType } from '@/types/dashboard'
import { EmptyState, ErrorState } from '../shared'
import type { SingleValueMappingConfig } from '@/lib/dataMapping'
import { normalizeDataSource } from '@/types/dashboard'
import {
  getEffectiveAggregate,
  getEffectiveTimeWindow,
  timeWindowToHours,
} from '@/lib/telemetryTransform'

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

  // Memoize calculations to prevent unnecessary recalculations
  const { min, max, isFlatLine, range, points } = useMemo(() => {
    const min = Math.min(...chartData)
    const max = Math.max(...chartData)
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

  return (
    <div ref={containerRef} className={cn('w-full h-full relative', className)}>
      <svg
        width="100%"
        height="100%"
        viewBox={`0 0 ${VIEWBOX_WIDTH} ${VIEWBOX_HEIGHT}`}
        preserveAspectRatio="none"
        style={{ overflow: 'visible' }}
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

export function Sparkline({
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
  className,
}: SparklineProps) {
  // Get effective aggregate and time window from dataSource or props
  const effectiveAggregate = useMemo(() => {
    const sources = normalizeDataSource(dataSource)
    if (sources.length > 0 && sources[0].aggregateExt) {
      return sources[0].aggregateExt
    }
    return aggregate
  }, [dataSource, aggregate])

  const effectiveTimeWindow = useMemo(() => {
    const sources = normalizeDataSource(dataSource)
    if (sources.length > 0 && sources[0].timeWindow?.type) {
      return sources[0].timeWindow.type
    }
    return timeWindow ?? 'last_1hour'
  }, [dataSource, timeWindow])

  // Normalize data sources to telemetry type with transform settings
  const telemetrySources = useMemo(() => {
    const sources = normalizeDataSource(dataSource)
    const timeRange = timeWindowToHours(effectiveTimeWindow)

    // Determine aggregate value with proper type
    const aggregateValue: 'raw' | 'avg' | 'min' | 'max' | 'sum' = effectiveAggregate === 'raw' ? 'raw' : 'avg'

    return sources.map(ds => {
      // If already telemetry type, preserve existing settings
      if (ds.type === 'telemetry') {
        return {
          ...ds,
          limit: ds.limit ?? 50,
          timeRange: ds.timeRange ?? timeRange,
          aggregate: ds.aggregate ?? aggregateValue,
          params: {
            ...ds.params,
            includeRawPoints: true,
          },
        }
      }

      // Convert device type to telemetry for historical data
      // Note: metric type without deviceId should NOT be converted as it won't match events
      if (ds.type === 'device' && ds.deviceId) {
        return {
          type: 'telemetry' as const,
          deviceId: ds.deviceId,
          metricId: ds.metricId ?? ds.property ?? 'value',
          timeRange: timeRange,
          limit: ds.limit ?? 50,
          aggregate: aggregateValue,
          params: {
            includeRawPoints: true,
          },
        }
      }

      return ds
    })
  }, [dataSource, effectiveAggregate, effectiveTimeWindow])

  // Use telemetry sources if available, otherwise use original dataSource
  const finalDataSource = telemetrySources.length > 0
    ? (telemetrySources.length === 1 ? telemetrySources[0] : telemetrySources)
    : dataSource

  // Fetch data with proper array handling
  // Don't use fallback for sparkline to avoid showing stale data during drag
  const { data, loading, error } = useDataSource<unknown>(finalDataSource, {
    preserveMultiple: true,
  })

  // Check if dataSource is configured
  const hasDataSource = dataSource !== undefined

  // Convert data to number array using the updated toNumberArray function
  const chartData = useMemo(() => {
    if (error) return []

    // Use propData only when there's no dataSource (static mode)
    // When dataSource exists, always use live data to avoid stale data during drag
    let rawData = hasDataSource ? data : propData
    if (Array.isArray(rawData) && rawData.length > 0 && Array.isArray(rawData[0])) {
      // Multi-source detected: combine all sources into one array
      // For sparkline, we interleave or append data from all sources
      const allData: unknown[] = []
      for (const sourceData of rawData) {
        if (Array.isArray(sourceData)) {
          allData.push(...sourceData)
        }
      }
      rawData = allData.length > 0 ? allData : rawData
    }

    const result = toNumberArray(rawData, [])
    // Only use DEFAULT_SAMPLE_DATA if there's no dataSource configured
    if (result.length === 0 && !hasDataSource) {
      return DEFAULT_SAMPLE_DATA
    }

    return result
  }, [data, propData, error, hasDataSource])

  const sizeConfig = dashboardComponentSize[size]

  // Calculate chart height based on size
  const chartHeight = height ?? (size === 'sm' ? 40 : size === 'md' ? 60 : 80)

  // Memoize stats to prevent recalculation on every render
  const stats = useMemo(() => {
    const latestValue = chartData.length > 0 ? chartData[chartData.length - 1] : 0
    const dataMax = chartData.length > 0 ? Math.max(...chartData) : 0
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

  // Inner content component
  const SparklineContent = () => (
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

  // Error state - use unified ErrorState
  if (error) {
    return <ErrorState size={size} className={className} />
  }

  // Empty state - use unified EmptyState (when dataSource is configured but no data available)
  if (hasDataSource && chartData.length < 2) {
    return <EmptyState size={size} className={className} message={title ? `${title} - No Data Available` : undefined} />
  }

  // Card wrapper mode (default for dashboard use)
  if (showCard) {
    return (
      <div className={cn(dashboardCardBase, 'flex flex-col', sizeConfig.padding, className)}>
        <SparklineContent />
      </div>
    )
  }

  // Non-card mode (when used in custom layouts)
  return (
    <div className={cn('flex flex-col w-full', className)}>
      <SparklineContent />
    </div>
  )
}
