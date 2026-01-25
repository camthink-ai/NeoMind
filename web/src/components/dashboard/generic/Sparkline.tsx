/**
 * Sparkline Component (Unified Styles)
 *
 * A shadcn/ui compliant mini chart for displaying trends.
 * Supports data binding and real-time updates.
 * Fully responsive and adaptive with comprehensive error handling.
 */

import { useRef, useEffect, useState } from 'react'
import { cn } from '@/lib/utils'
import { Skeleton } from '@/components/ui/skeleton'
import { useDataSource, useNumberArrayDataSource } from '@/hooks/useDataSource'
import { dashboardComponentSize, dashboardCardBase, dashboardCardContent } from '@/design-system/tokens/size'
import {
  indicatorFontWeight,
  indicatorColors,
  getGradientStops,
  getValueStateColor,
  getValueGradient,
  type IndicatorState,
  type IndicatorGradientType,
} from '@/design-system/tokens/indicator'
import { statusColors } from '@/design-system/tokens/color'
import type { DataSourceOrList } from '@/types/dashboard'

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
  colorMode?: 'auto' | 'primary' | 'fixed' | 'value'  // value: like ProgressBar (based on latest value/max ratio)
  fill?: boolean
  fillColor?: string
  showPoints?: boolean
  strokeWidth?: number
  curved?: boolean

  // Threshold line
  showThreshold?: boolean
  threshold?: number
  thresholdColor?: string

  // Value range for value-based coloring
  maxValue?: number  // For colorMode='value', determines color based on latestValue/maxValue ratio

  // Value display
  showValue?: boolean
  label?: string
  size?: 'sm' | 'md' | 'lg'

  className?: string
}

// Get trend-based color
function getTrendColor(trend: number): string {
  if (trend > 0) return indicatorColors.success.base
  if (trend < 0) return indicatorColors.error.base
  return indicatorColors.neutral.base
}

// Get trend-based text color class
function getTrendTextColor(trend: number): string {
  if (trend > 0) return indicatorColors.success.text
  if (trend < 0) return indicatorColors.error.text
  return indicatorColors.neutral.text
}

/**
 * Safely convert to number array
 */
function safeToNumberArray(data: unknown): number[] {
  if (Array.isArray(data)) {
    return data
      .map((v) => typeof v === 'number' ? v : typeof v === 'string' ? parseFloat(v) : 0)
      .filter((v) => !isNaN(v))
  }
  return []
}

// Internal sparkline component that tracks container size
function ResponsiveSparkline({
  data: chartData,
  width: initialWidth,
  height,
  color,
  fill,
  fillColor,
  showPoints,
  strokeWidth,
  curved,
  showThreshold,
  threshold,
  thresholdColor,
  gradientType,  // Add gradient type for proper coloring
  className,
}: {
  data: number[]
  width: number
  height: number
  color: string
  fill?: boolean
  fillColor?: string
  showPoints?: boolean
  strokeWidth?: number
  curved?: boolean
  showThreshold?: boolean
  threshold?: number
  thresholdColor?: string
  gradientType?: IndicatorGradientType
  className?: string
}) {
  const containerRef = useRef<HTMLDivElement>(null)
  const [containerWidth, setContainerWidth] = useState(initialWidth)

  // Track container size for responsiveness (width only for responsiveness)
  useEffect(() => {
    const updateSize = () => {
      if (containerRef.current) {
        const newWidth = containerRef.current.offsetWidth
        setContainerWidth(newWidth > 0 ? newWidth : initialWidth)
      }
    }

    updateSize()

    const resizeObserver = new ResizeObserver(updateSize)
    if (containerRef.current) {
      resizeObserver.observe(containerRef.current)
    }

    return () => resizeObserver.disconnect()
  }, [initialWidth])

  const min = Math.min(...chartData)
  const max = Math.max(...chartData)
  const isFlatLine = max === min
  const range = max - min || 1

  const width = containerWidth

  // Calculate points - center flat lines vertically for better aesthetics
  const points = chartData.map((v, i) => {
    const x = (i / (chartData.length - 1)) * width
    const y = isFlatLine
      ? height / 2  // Center flat lines vertically
      : height - ((v - min) / range) * height
    return { x, y, value: v }
  })

  // Create path string
  let pathD: string
  if (curved && points.length > 2) {
    // Create curved path using bezier curves
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

    pathD = curvePoints.join(' ')
  } else {
    // Linear path
    pathD = points.map((p, i) => `${i === 0 ? 'M' : 'L'} ${p.x} ${p.y}`).join(' ')
  }

  // Create fill path
  const fillPath = `${pathD} L ${width} ${height} L 0 ${height} Z`

  // Unique gradient ID for this instance
  const gradientId = `sparkline-gradient-${color.replace(/[^a-zA-Z0-9]/g, '')}`

  // Get gradient stops based on gradient type
  const gradientStops = getGradientStops(gradientType || 'primary', color)

  return (
    <div ref={containerRef} className={cn('w-full h-full flex items-center justify-center overflow-visible', className)}>
      <svg
        width={containerWidth}
        height={height}
        viewBox={`0 0 ${containerWidth} ${height}`}
        preserveAspectRatio="none"
        style={{ overflow: 'visible' }}
      >
        <defs>
          {/* Enhanced gradient for fill area - using unified gradient system */}
          <linearGradient id={gradientId} x1="0%" y1="0%" x2="0%" y2="100%">
            {gradientStops.map((stop, i) => (
              <stop
                key={i}
                offset={stop.offset}
                stopColor={stop.color}
                stopOpacity={stop.opacity + 0.3}
              />
            ))}
          </linearGradient>

          {/* Glow filter for the line */}
          <filter id={`glow-${gradientId}`} x="-50%" y="-50%" width="200%" height="200%">
            <feGaussianBlur stdDeviation="1.5" result="coloredBlur" />
            <feMerge>
              <feMergeNode in="coloredBlur" />
              <feMergeNode in="SourceGraphic" />
            </feMerge>
          </filter>
        </defs>

        {/* Fill area with enhanced gradient */}
        {fill && (
          <path
            d={fillPath}
            fill={`url(#${gradientId})`}
            className="transition-opacity duration-300"
          />
        )}

        {/* Main line with glow effect */}
        <path
          d={pathD}
          fill="none"
          stroke={color}
          strokeWidth={strokeWidth}
          strokeLinecap="round"
          strokeLinejoin="round"
          vectorEffect="non-scaling-stroke"
          filter={`url(#glow-${gradientId})`}
          className="transition-all duration-300"
        />

        {/* Optional points */}
        {showPoints && points.map((p, i) => (
          <circle
            key={i}
            cx={p.x}
            cy={p.y}
            r={2.5}
            fill={color}
            className="opacity-50 transition-opacity duration-200 hover:opacity-100"
          />
        ))}

        {/* Threshold line */}
        {showThreshold && threshold !== undefined && !isFlatLine && (
          <line
            x1={0}
            y1={height - ((threshold - min) / range) * height}
            x2={width}
            y2={height - ((threshold - min) / range) * height}
            stroke={thresholdColor}
            strokeWidth={1.5}
            strokeDasharray="4 4"
            vectorEffect="non-scaling-stroke"
            className="opacity-60"
          />
        )}

        {/* Last value indicator with enhanced glow */}
        <g className="animate-pulse">
          {/* Outer glow ring */}
          <circle
            cx={points[points.length - 1].x}
            cy={points[points.length - 1].y}
            r={6}
            fill={color}
            fillOpacity="0.2"
          />
          {/* Main dot */}
          <circle
            cx={points[points.length - 1].x}
            cy={points[points.length - 1].y}
            r={3.5}
            fill={color}
            className="stroke-background stroke-1"
          />
        </g>
      </svg>
    </div>
  )
}

export function Sparkline({
  dataSource,
  data: propData,
  width = 100,
  height = 40,
  responsive = false,
  showCard = false,
  color,
  colorMode = 'auto',  // Default to trend-based coloring
  fill = true,
  fillColor,
  showPoints = false,
  strokeWidth = 2,
  curved = true,
  showThreshold = false,
  threshold,
  thresholdColor,
  maxValue,
  showValue = false,
  label,
  size = 'md',
  className,
}: SparklineProps) {
  // Get data from source with proper array handling
  const { data, loading, error } = useNumberArrayDataSource(dataSource, {
    fallback: propData ?? [],
  })

  // Ensure data is a valid number array
  const chartData = safeToNumberArray(error ? [] : data ?? propData ?? [])

  const sizeConfig = dashboardComponentSize[size]

  // Calculate stats
  const latestValue = chartData.length > 0 ? chartData[chartData.length - 1] : 0
  const prevValue = chartData.length > 1 ? chartData[chartData.length - 2] : latestValue
  const trend = chartData.length > 1 ? latestValue - prevValue : 0
  const trendPercent = prevValue !== 0 ? ((trend / prevValue) * 100).toFixed(1) : '0'

  // For value-based coloring, determine the max value for ratio calculation
  const dataMax = chartData.length > 0 ? Math.max(...chartData) : 0
  const effectiveMax = maxValue ?? dataMax ?? 100

  // Determine line color based on colorMode
  const lineColor = colorMode === 'auto'
    ? (color || getTrendColor(trend))
    : colorMode === 'value'
      ? (color || getValueStateColor(latestValue, effectiveMax))
      : colorMode === 'primary'
        ? indicatorColors.primary.base
        : color || indicatorColors.primary.base

  // Determine text color for trend display
  const trendTextColor = colorMode === 'auto'
    ? getTrendTextColor(trend)
    : indicatorColors.primary.text

  // Determine gradient state for all modes
  let gradientState: IndicatorGradientType = 'neutral'
  if (colorMode === 'value') {
    gradientState = getValueGradient(latestValue, effectiveMax)
  } else if (colorMode === 'primary') {
    gradientState = 'primary'
  } else if (colorMode === 'auto') {
    gradientState = trend > 0 ? 'success' : trend < 0 ? 'error' : 'neutral'
  }

  if (loading) {
    return (
      <div className={cn(dashboardCardBase, 'flex items-center justify-center', sizeConfig.padding, className)}>
        <Skeleton className="w-full h-full" />
      </div>
    )
  }

  if (error) {
    return (
      <div className={cn(dashboardCardBase, 'flex items-center justify-center', sizeConfig.padding, className)}>
        <span className={cn('text-destructive/60', sizeConfig.labelText)}>Error loading data</span>
      </div>
    )
  }

  if (chartData.length < 2) {
    return (
      <div className={cn(dashboardCardBase, 'flex items-center justify-center', sizeConfig.padding, className)}>
        <span className={cn('text-muted-foreground/50', sizeConfig.labelText)}>No data</span>
      </div>
    )
  }

  // Card wrapper mode - unified styling
  if (showCard) {
    return (
      <div className={cn(dashboardCardBase, 'flex flex-col', sizeConfig.padding, className)}>
        {/* Header with label and value */}
        {(label || showValue) && (
          <div className="flex items-center justify-between mb-2">
            {label && (
              <span className={cn(indicatorFontWeight.title, 'text-muted-foreground', sizeConfig.labelText)}>
                {label}
              </span>
            )}
            {showValue && (
              <div className="flex items-center gap-2">
                <span className={cn(indicatorFontWeight.value, 'text-foreground tabular-nums', sizeConfig.valueText)}>
                  {latestValue.toLocaleString(undefined, { maximumFractionDigits: 1 })}
                </span>
                {trend !== 0 && (
                  <span className={cn(
                    indicatorFontWeight.meta,
                    'text-xs',
                    trend > 0 ? indicatorColors.success.text : indicatorColors.error.text
                  )}>
                    {trend > 0 ? '+' : ''}{trendPercent}%
                  </span>
                )}
              </div>
            )}
          </div>
        )}

        {/* Chart */}
        <div className={cn('flex-1 min-h-0', 'overflow-visible')}>
          <ResponsiveSparkline
            data={chartData}
            width={width}
            height={height}
            color={lineColor}
            fill={fill}
            fillColor={fillColor}
            showPoints={showPoints}
            strokeWidth={strokeWidth}
            curved={curved}
            showThreshold={showThreshold}
            threshold={threshold}
            thresholdColor={thresholdColor || indicatorColors.neutral.base}
            gradientType={gradientState}
          />
        </div>
      </div>
    )
  }

  if (responsive) {
    return (
      <div className={cn('w-full h-full overflow-visible', className)}>
        <ResponsiveSparkline
          data={chartData}
          width={width}
          height={height}
          color={lineColor}
          fill={fill}
          fillColor={fillColor}
          showPoints={showPoints}
          strokeWidth={strokeWidth}
          curved={curved}
          showThreshold={showThreshold}
          threshold={threshold}
          thresholdColor={thresholdColor || indicatorColors.neutral.base}
          gradientType={gradientState}
          className="flex items-center justify-center"
        />
      </div>
    )
  }

  // Non-responsive mode - original behavior with enhanced styling
  const min = Math.min(...chartData)
  const max = Math.max(...chartData)
  const isFlatLine = max === min
  const range = max - min || 1

  const points = chartData.map((v, i) => {
    const x = (i / (chartData.length - 1)) * width
    const y = isFlatLine
      ? height / 2  // Center flat lines vertically
      : height - ((v - min) / range) * height
    return { x, y, value: v }
  })

  let pathD: string
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

    pathD = curvePoints.join(' ')
  } else {
    pathD = points.map((p, i) => `${i === 0 ? 'M' : 'L'} ${p.x} ${p.y}`).join(' ')
  }

  const fillPath = `${pathD} L ${width} ${height} L 0 ${height} Z`

  // Use unified gradient system (gradientState calculated above)
  const gradientStops = getGradientStops(gradientState, lineColor)
  const gradientId = `sparkline-static-${lineColor.replace(/[^a-zA-Z0-9]/g, '')}`

  return (
    <div className={cn('overflow-visible', className)}>
      <svg
        width={width}
        height={height}
        viewBox={`0 0 ${width} ${height}`}
        preserveAspectRatio="none"
        style={{ overflow: 'visible', display: 'block' }}
      >
      <defs>
        <linearGradient id={gradientId} x1="0%" y1="0%" x2="0%" y2="100%">
          {gradientStops.map((stop, i) => (
            <stop
              key={i}
              offset={stop.offset}
              stopColor={stop.color}
              stopOpacity={stop.opacity + 0.3}
            />
          ))}
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
        stroke={lineColor}
        strokeWidth={strokeWidth}
        strokeLinecap="round"
        strokeLinejoin="round"
        filter={`url(#glow-${gradientId})`}
      />
      {showThreshold && threshold !== undefined && !isFlatLine && (
        <line
          x1={0}
          y1={height - ((threshold - min) / range) * height}
          x2={width}
          y2={height - ((threshold - min) / range) * height}
          stroke={thresholdColor || indicatorColors.neutral.base}
          strokeWidth={1.5}
          strokeDasharray="4 4"
          className="opacity-60"
        />
      )}
      <g className="animate-pulse">
        <circle
          cx={points[points.length - 1].x}
          cy={points[points.length - 1].y}
          r={5}
          fill={lineColor}
          fillOpacity="0.2"
        />
        <circle
          cx={points[points.length - 1].x}
          cy={points[points.length - 1].y}
          r={3}
          fill={lineColor}
        />
      </g>
    </svg>
    </div>
  )
}
