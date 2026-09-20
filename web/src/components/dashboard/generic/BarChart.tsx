/**
 * Bar Chart Component
 *
 * Unified with dashboard design system.
 * Supports historical telemetry data binding.
 *
 * Enhanced with time-series aggregation support:
 * - Aggregate multiple data points into single values
 * - Support for different aggregation methods (latest, avg, sum, etc.)
 * - Time window selection for data scope
 * - Chart view modes: timeseries, snapshot, comparison
 */

import { useMemo, memo, useId } from 'react'
import { useTranslation } from 'react-i18next'

import {
  BarChart as RechartsBarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
  Cell,
} from 'recharts'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'
import { DataMapper, type CategoricalMappingConfig } from '@/lib/dataMapping'
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
import { indicatorFontWeight } from '@/design-system/tokens/indicator'
import { chartColors as designChartColors, chartColorsHex } from '@/design-system/tokens/color'
import type { DataSourceOrList, TelemetryAggregate } from '@/types/dashboard'
import { ChartContainer, ChartTooltip, EmptyState, useChartDimensions, useStaggeredData, createMemoRenderer, useChartPipeline } from '../shared'
import { isNameValueData, isNumberArray, isMultiSourceData } from '../shared'
import {
  createChartTimeFormatter,
  aggregateData,
} from '@/lib/telemetryTransform'
import type { TimePoint } from '@/lib/telemetryTransform'

// Use design system chart colors
const _chartColors = designChartColors

// Fallback colors as hex values for SVG
const fallbackColors = chartColorsHex

/**
 * Transform raw telemetry points to chart data using DataMapper
 * - 'count': groups by value and counts occurrences
 * - 'raw': shows all time-series points as bars
 * - 'latest'/'avg'/'sum'/etc.: aggregates to a single value
 */
function transformTelemetryToBarData(
  data: unknown,
  dataMapping?: CategoricalMappingConfig,
  aggregate?: TelemetryAggregate,
  label?: string
): { name: string; value: number; color?: string }[] {
  if (!data || !Array.isArray(data)) return []

  // Handle simple number array
  if (data.length > 0 && typeof data[0] === 'number') {
    return (data as number[]).map((value, index) => ({
      name: `${index + 1}`,
      value,
    }))
  }

  // Check if already in BarData format FIRST (has both name and value)
  if (data.length > 0 && typeof data[0] === 'object' && data[0] !== null && 'value' in data[0] && 'name' in data[0]) {
    return data as { name: string; value: number; color?: string }[]
  }

  // Handle raw telemetry points format: [{ timestamp, value }, ...]
  // Only has 'value' but not 'name' → this is telemetry data
  if (data.length > 0 && typeof data[0] === 'object' && data[0] !== null && 'value' in data[0]) {
    // Data is already sorted ascending (oldest-first) by useDataSource pipeline
    const telemetryPoints = data as Array<{ timestamp?: number; value: unknown }>

    // Check if values are categorical (strings) OR if aggregate is 'count' - group and count for distribution
    const isCategorical = telemetryPoints.some((p) => typeof p.value === 'string')
    const forceCounting = aggregate === 'count'

    if (isCategorical || forceCounting) {
      // Group by value and count occurrences for categorical data or when 'count' aggregate is set
      const counts = new Map<string, number>()
      for (const point of telemetryPoints) {
        // For numeric values with counting, group by rounded value to avoid too many unique values
        let key: string
        if (typeof point.value === 'number') {
          // Round to 2 decimal places for grouping
          key = String(Math.round(point.value * 100) / 100)
        } else {
          key = String(point.value ?? 'unknown')
        }
        counts.set(key, (counts.get(key) ?? 0) + 1)
      }

      // Convert to BarData format, sorted by count descending
      return Array.from(counts.entries())
        .sort((a, b) => b[1] - a[1])
        .map(([name, value]) => ({
          name,
          value,
        }))
    }

    // For non-raw aggregates (latest, avg, sum, min, max, etc.), aggregate to single value
    if (aggregate && aggregate !== 'raw') {
      const now = Date.now() / 1000
      const timePoints: TimePoint[] = telemetryPoints.map((p, idx) => ({
        timestamp: p.timestamp ?? (now - (telemetryPoints.length - idx) * 60),
        value: typeof p.value === 'number' ? p.value : 0,
      }))
      const aggregated = aggregateData(timePoints, aggregate)
      if (aggregated !== null) {
        return [{ name: label || 'Value', value: aggregated }]
      }
    }

    // For raw aggregate, show all time-series points as bars
    const timestamps = telemetryPoints.map(p => p.timestamp).filter((t): t is number => t !== undefined)
    const fmtTime = createChartTimeFormatter(timestamps)
    return telemetryPoints.map((point, index) => {
      let name = `${index + 1}`
      if (point.timestamp) {
        const formatted = fmtTime(point.timestamp)
        if (formatted) name = formatted
      }
      return {
        name,
        value: typeof point.value === 'number' ? point.value : 0,
      }
    })
  }

  // Use DataMapper for categorical data
  const categoricalPoints = DataMapper.mapToCategorical(data, dataMapping)

  return categoricalPoints.map(p => ({
    name: p.name,
    value: p.value,
    color: p.color,
  }))
}

export interface BarData {
  name: string
  value: number
  color?: string
  timestamp?: number  // For time-series mode
}

export interface BarChartProps {
  // Data source configuration
  dataSource?: DataSourceOrList  // Support both single and multiple data sources

  // Data
  data?: BarData[]

  // Display options
  title?: string
  height?: number | 'auto'
  showGrid?: boolean
  showLegend?: boolean
  showTooltip?: boolean

  // Layout
  layout?: 'vertical' | 'horizontal'

  // === Telemetry options ===
  // Legacy options (kept for backward compatibility)
  limit?: number
  timeRange?: number

  // === New: Data transformation options ===
  // How to aggregate time-series data
  aggregate?: TelemetryAggregate
  // Time window for data
  timeWindow?: 'now' | 'last_5min' | 'last_15min' | 'last_30min' | 'last_1hour' | 'last_6hours' | 'last_24hours'

  // Data mapping configuration
  dataMapping?: CategoricalMappingConfig

  // Styling
  color?: string
  size?: 'sm' | 'md' | 'lg'
  className?: string
}

export const BarChart = memo(function BarChart({
  dataSource,
  data: propData,
  title,
  height: _height = 'auto',
  showGrid = false,
  showLegend = false,
  showTooltip = true,
  layout: _layout = 'vertical',
  color,
  size = 'md',
  limit = 24,
  timeRange = 1,
  aggregate = 'raw',  // Default to raw for bar charts (show time series)
  timeWindow: _timeWindow,
  dataMapping,
  className,
}: BarChartProps) {
  const { t } = useTranslation('dashboardComponents')
  const config = dashboardComponentSize[size]

  // Shared data pipeline
  const {
    sources, data, loading, effectiveAggregate,
    hasData: _hasData, showLoading, getSeriesName,
  } = useChartPipeline<BarData[] | number[] | number[][]>({
    dataSource,
    aggregate,
    limit,
    timeRange,
    fallback: undefined,
    preserveMultiple: true,
  })

  // Multi-source chart data: one bar per source with aggregated value (like PieChart)
  const multiSourceChartData = useMemo(() => {
    if (!isMultiSourceData(data, sources.length)) return null

    const sourceArrays = data as unknown[][]
    const _chartColors = color ? [color] : fallbackColors

    return sources.map((ds, i) => {
      const arr = sourceArrays[i]
      const seriesName = getSeriesName(ds, i)

      if (!Array.isArray(arr) || arr.length === 0) {
        return { name: seriesName, value: 0 }
      }

      // Check if data contains telemetry points with value property
      const firstItem = arr[0] as { value?: unknown; timestamp?: number } | undefined
      const hasTelemetryPoints = typeof firstItem === 'object' && firstItem !== null && 'value' in firstItem

      if (hasTelemetryPoints) {
        // Extract values from telemetry points
        const values = (arr as Array<{ value?: unknown }>).map(item => {
          const val = item.value
          return typeof val === 'number' ? val : parseFloat(String(val)) || 0
        }).filter(v => !isNaN(v))

        if (values.length > 0) {
          // Create timePoints for aggregation
          const now = Date.now() / 1000
          const timePoints = (arr as Array<{ value?: unknown; timestamp?: number }>).map((item, idx) => ({
            timestamp: item.timestamp || (now - (values.length - idx) * 60),
            value: values[idx],
          }))

          const aggregatedValue = aggregateData(timePoints, effectiveAggregate)
          return { name: seriesName, value: aggregatedValue ?? 0 }
        }
      }

      // Handle simple number array
      const numericValues = (arr as unknown[]).filter(v => typeof v === 'number')
      if (numericValues.length > 0) {
        const now = Date.now() / 1000
        const timePoints = numericValues.map((v, idx) => ({
          timestamp: now - (numericValues.length - idx) * 60,
          value: v as number,
        }))
        const aggregatedValue = aggregateData(timePoints, effectiveAggregate)
        return { name: seriesName, value: aggregatedValue ?? 0 }
      }

      return { name: seriesName, value: 0 }
    })
  }, [data, sources, effectiveAggregate, getSeriesName, color])

  // Single-source chart data
  const singleSourceChartData = useMemo(() => {
    if (dataSource && isNameValueData(data)) {
      return data as BarData[]
    }

    if (dataSource && Array.isArray(data) && data.length > 0 && !isNameValueData(data)) {
      const seriesName = sources[0] ? getSeriesName(sources[0], 0) : title
      const transformed = transformTelemetryToBarData(data, dataMapping, effectiveAggregate, seriesName)
      if (transformed.length > 0) return transformed
    }

    if (dataSource && isNumberArray(data)) {
      return (data as number[]).map((value, index) => ({
        name: `${index + 1}`,
        value,
      }))
    }

    if (!dataSource && propData && Array.isArray(propData) && propData.length > 0) {
      return propData
    }

    if (dataSource && !loading && Array.isArray(data) && data.length === 0) {
      return []
    }

    // Default sample data for preview
    return [
      { name: t('chart.jan'), value: 12 },
      { name: t('chart.feb'), value: 18 },
      { name: t('chart.mar'), value: 15 },
      { name: t('chart.apr'), value: 22 },
      { name: t('chart.may'), value: 19 },
      { name: t('chart.jun'), value: 25 },
    ]
  }, [data, propData, dataSource, loading, dataMapping, effectiveAggregate, sources, getSeriesName, t])

  // Select between multi-source and single-source
  const chartData = multiSourceChartData ?? singleSourceChartData

  // Multi-source now uses standard { name, value } format, no grouped series needed
  const seriesInfo = useMemo(() => null, [])

  // Show loading skeleton when fetching data
  if (dataSource && showLoading) {
    return (
      <div className={cn(dashboardCardBase, 'h-full flex flex-col', config.padding, className)}>
        {title && (
          <div className={cn('mb-3', indicatorFontWeight.title, config.titleText)}>{title}</div>
        )}
        <Skeleton className={cn('w-full', 'flex-1 min-h-0')} />
      </div>
    )
  }

  // Only show empty state when loading is fully complete
  if (!loading && chartData.length === 0) {
    return <EmptyState size={size} className={className} message={title ? `${title} - No Data Available` : undefined} />
  }

  return (
    <div className={cn(dashboardCardBase, config.padding, className)}>
      {title && (
        <div className={cn('mb-3', indicatorFontWeight.title, config.titleText)}>{title}</div>
      )}
      <ChartContainer>
        <BarChartWithDimensions data={chartData} seriesInfo={seriesInfo} showGrid={showGrid} showTooltip={showTooltip} showLegend={showLegend} color={color} chartData={chartData} />
      </ChartContainer>
    </div>
  )
})

const BarChartRenderer = createMemoRenderer(({ data, seriesInfo, showGrid, showTooltip, showLegend, color, chartData, width, height, uid }: {
  data: any[]
  seriesInfo: { dataKey: string; name: string; color: string }[] | null
  showGrid: boolean
  showTooltip: boolean
  showLegend: boolean
  color?: string
  chartData: any[]
  width: number
  height: number
  uid: string
}) => {
  const fallbackColors = chartColorsHex
  return (
    <RechartsBarChart width={width} height={height} data={data} margin={{ top: 5, right: 5, bottom: 0, left: 0 }}>
      <defs>
        {(seriesInfo ? seriesInfo.map(s => s.color) : [color || fallbackColors[0]]).map((c, i) => (
          <linearGradient key={i} id={`bar-grad-${uid}-${i}`} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor={c} stopOpacity={1} />
            <stop offset="100%" stopColor={c} stopOpacity={0.65} />
          </linearGradient>
        ))}
      </defs>
      {showGrid && <CartesianGrid vertical={false} strokeDasharray="4 4" className="stroke-muted" />}
      <XAxis dataKey="name" axisLine={false} tickLine={false} tickMargin={10} tick={{ fill: 'var(--muted-foreground)', fontSize: 10 }} interval="preserveStartEnd" />
      <YAxis axisLine={false} tickLine={false} tickMargin={10} width={32} tick={{ fill: 'var(--muted-foreground)', fontSize: 10 }} />
      {showTooltip && <Tooltip content={<ChartTooltip />} />}
      {showLegend && <Legend />}
      {seriesInfo ? (
        seriesInfo.map((info, i) => (
          <Bar key={info.dataKey} dataKey={info.dataKey} name={info.name} fill={`url(#bar-grad-${uid}-${i})`} radius={4} isAnimationActive animationDuration={600} />
        ))
      ) : (
        <Bar dataKey="value" fill={`url(#bar-grad-${uid}-0)`} radius={4} isAnimationActive animationDuration={600}>
          {chartData.some(d => d.color) && chartData.map((entry, index) => (
            <Cell key={`cell-${index}`} fill={entry.color || color || fallbackColors[index % fallbackColors.length]} />
          ))}
        </Bar>
      )}
    </RechartsBarChart>
  )
})

function BarChartWithDimensions({ data, seriesInfo, showGrid, showTooltip, showLegend, color, chartData }: {
  data: any[]
  seriesInfo: { dataKey: string; name: string; color: string }[] | null
  showGrid: boolean
  showTooltip: boolean
  showLegend: boolean
  color?: string
  chartData: any[]
}) {
  const { ref, width, height, turn } = useChartDimensions()
  const staggeredData = useStaggeredData(data, turn)
  const staggeredSeriesInfo = useStaggeredData(seriesInfo, turn)
  const staggeredChartData = useStaggeredData(chartData, turn)
  const uid = useId().replace(/:/g, '')
  return (
    <div ref={ref} style={{ width: '100%', height: '100%' }}>
      {width > 0 && height > 0 && (
        <BarChartRenderer uid={uid} data={staggeredData} seriesInfo={staggeredSeriesInfo} showGrid={showGrid} showTooltip={showTooltip} showLegend={showLegend} color={color} chartData={staggeredChartData} width={width} height={height} />
      )}
    </div>
  )
}
