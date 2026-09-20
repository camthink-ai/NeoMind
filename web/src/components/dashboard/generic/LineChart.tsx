/**
 * Line Chart Component
 *
 * Unified with dashboard design system.
 * Supports historical telemetry data binding.
 *
 * Enhanced with time-series aggregation support:
 * - Aggregate multiple data points into single values
 * - Support for different aggregation methods (latest, avg, sum, etc.)
 * - Time window selection for data scope
 * - Chart view modes: timeseries, snapshot
 */

import { useMemo, memo, useId } from 'react'
import { useTranslation } from 'react-i18next'
import {
  LineChart as RechartsLineChart,
  AreaChart as RechartsAreaChart,
  Line,
  Area,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
} from 'recharts'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'
import { DataMapper, type TimeSeriesMappingConfig } from '@/lib/dataMapping'
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
import { indicatorFontWeight } from '@/design-system/tokens/indicator'
import { chartColors as designChartColors, chartColorsHex } from '@/design-system/tokens/color'
import type { DataSourceOrList, TelemetryAggregate } from '@/types/dashboard'
import { ChartContainer, ChartTooltip, EmptyState, ErrorState, useChartDimensions, useStaggeredData, createMemoRenderer, useChartPipeline } from '../shared'
import { isSeriesDataArray, isNumberArray, isMultiSourceData, alignMultiSource, extractNumericValue, type SeriesData } from '../shared'
import {
  createChartTimeFormatter,
} from '@/lib/telemetryTransform'

// Use design system chart colors
const _chartColors = designChartColors

// Use design system hex colors for SVG rendering
const fallbackColors = chartColorsHex

/**
 * Transform raw telemetry points to chart data using DataMapper
 * Handles formats: [{ timestamp, value }, { t, v }, { time, val }] or number arrays
 * Converts string values to numbers when possible
 */
function transformTelemetryToChartData(
  data: unknown,
  dataMapping?: TimeSeriesMappingConfig
): { labels: string[]; values: number[] } {
  // Empty data
  if (!data) return { labels: [], values: [] }

  // Array of telemetry points - use DataMapper for time series
  if (Array.isArray(data)) {
    // Check if it's already in simple number array format
    if (data.length > 0 && typeof data[0] === 'number') {
      return {
        labels: data.map((_, i) => `${i + 1}`),
        values: data as number[],
      }
    }

    // Check if it's already in SeriesData format
    if (data.length > 0 && typeof data[0] === 'object' && data[0] !== null && 'data' in data[0]) {
      const seriesData = data[0] as SeriesData
      return {
        labels: seriesData.data.map((_, i) => `${i + 1}`),
        values: seriesData.data,
      }
    }

    // Use DataMapper to map to time series
    // Data is already sorted ascending (oldest-first) by useDataSource pipeline
    const timeSeriesPoints = DataMapper.mapToTimeSeries(data, dataMapping)

    // Extract values and format labels from timestamps
    const values = timeSeriesPoints.map(p => p.value)
    const timestamps = timeSeriesPoints.map(p => p.timestamp).filter((t): t is number => t !== undefined)
    const fmtTime = createChartTimeFormatter(timestamps)
    const labels = timeSeriesPoints.map((p, idx) => {
      if (p.timestamp) {
        const formatted = fmtTime(p.timestamp)
        if (formatted) return formatted
      }
      return p.label || `${idx + 1}`
    })

    return { labels, values }
  }

  return { labels: [], values: [] }
}

/**
 * Format timestamp to readable time
 */
function _formatTimestamp(timestamp: string | number | undefined): string {
  if (!timestamp) return ''

  const date = new Date(typeof timestamp === 'number' ? timestamp * 1000 : timestamp)
  if (isNaN(date.getTime())) return String(timestamp)

  return date.toLocaleTimeString('en-US', {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })
}

export interface LineChartProps {
  // Data source configuration
  dataSource?: DataSourceOrList  // Support both single and multiple data sources

  // Data
  series?: SeriesData[]
  labels?: string[]

  // Display options
  title?: string
  height?: number | 'auto'
  showGrid?: boolean
  showLegend?: boolean
  showTooltip?: boolean
  smooth?: boolean
  fillArea?: boolean
  color?: string
  size?: 'sm' | 'md' | 'lg'

  // === Telemetry options ===
  // Legacy options (kept for backward compatibility)
  limit?: number
  timeRange?: number

  // === New: Data transformation options ===
  // How to aggregate time-series data
  aggregate?: TelemetryAggregate

  // Data mapping configuration
  dataMapping?: TimeSeriesMappingConfig

  className?: string
}

const LineChartInner = function LineChart({
  dataSource,
  series: propSeries,
  labels: propLabels,
  title,
  height = 'auto',
  showGrid = false,
  showLegend = false,
  showTooltip = true,
  smooth = true,
  fillArea = false,
  color,
  size = 'md',
  limit = 50,
  timeRange = 1,
  aggregate = 'raw',  // Default to raw for line charts (show time series)
  dataMapping,
  className,
}: LineChartProps) {
  const { t } = useTranslation('dashboardComponents')
  const config = dashboardComponentSize[size]

  // Shared data pipeline
  const {
    sources, data, loading, error,
    hasData, showLoading, getSeriesName,
  } = useChartPipeline<any>({
    dataSource,
    aggregate,
    limit,
    timeRange,
    fallback: propSeries ?? [],
    preserveMultiple: true,
  })

  // Transform data to series format
  const normalizedSeries: SeriesData[] = useMemo(() => {

    // Multi-source case - data should be array of arrays from useDataSource with preserveMultiple
    if (isMultiSourceData(data, sources.length)) {
      const seriesResult = sources.map((ds, idx) => {
        const sourceData = idx < data.length ? data[idx] : []
        let values: number[] = []
        if (Array.isArray(sourceData)) {
          if (isNumberArray(sourceData)) {
            values = sourceData as number[]
          } else {
            const { values: v } = transformTelemetryToChartData(sourceData, dataMapping)
            values = v
          }
        }

        const seriesName = getSeriesName(ds, idx)
        return {
          name: seriesName,
          data: values,
          color: undefined,
        } as SeriesData
      })

      // If ALL series have empty data, fall through to fallback/sample data
      const hasAnyData = seriesResult.some(s => s.data.length > 0)
      if (hasAnyData) {
        return seriesResult
      }
      // Fall through to use fallback or sample data below
    }

    // Handle telemetry raw data FIRST (when dataSource is provided)
    if (dataSource && isSeriesDataArray(data)) {
      return data
    }

    if (dataSource && Array.isArray(data) && data.length > 0 && !isSeriesDataArray(data)) {

      // Single source - transform telemetry points
      const { labels, values } = transformTelemetryToChartData(data, dataMapping)
      if (values.length > 0) {
        const singleSource = sources[0]
        const seriesName = singleSource ? getSeriesName(singleSource, 0) : 'Value'
        return [{ name: seriesName, data: values, color: undefined } as SeriesData]
      }
    }

    // Handle single number from data source
    if (dataSource && typeof data === 'number') {
      return [{ name: 'Value', data: [data], color: undefined } as SeriesData]
    }

    // Handle number array from data source
    if (dataSource && isNumberArray(data)) {
      return [{ name: 'Value', data: data as number[], color: undefined } as SeriesData]
    }

    // If no dataSource, use propSeries (static data) — filter out items without data
    if (!dataSource && propSeries && Array.isArray(propSeries) && propSeries.length > 0) {
      const validSeries = propSeries.filter(s => Array.isArray(s?.data))
      if (validSeries.length > 0) return validSeries
    }

    // When dataSource is configured but produced no data, show empty state (not sample data)
    if (dataSource) return []

    // Default fallback (no dataSource = preview mode)
    return [{
      name: 'Sample',
      data: [10, 15, 12, 18, 14, 20, 16, 22, 19, 25],
      color: undefined,
    } as SeriesData]
  }, [data, propSeries, dataSource, dataMapping, sources, getSeriesName])

  // Extract timestamp-aligned data for multi-source, or sorted labels for single-source
  const { chartLabels, alignedSeries } = useMemo(() => {
    // --- Multi-source with timestamps: align by timestamp ---
    if (sources.length > 1 && isMultiSourceData(data, sources.length)) {
      const aligned = alignMultiSource(data, sources, getSeriesName, dataMapping)
      if (aligned) return { chartLabels: aligned.chartLabels, alignedSeries: aligned.series }
    }

    // --- Single source with timestamps ---
    if (dataSource && Array.isArray(data) && data.length > 0) {
      const first = data[0]
      if (typeof first === 'object' && first !== null && ('timestamp' in first || 't' in first || 'time' in first)) {
        const { labels: telemetryLabels, values } = transformTelemetryToChartData(data, dataMapping)
        if (telemetryLabels.length > 0) {
          const singleSource = sources[0]
          const seriesName = singleSource ? getSeriesName(singleSource, 0) : 'Value'
          return {
            chartLabels: telemetryLabels,
            alignedSeries: [{ name: seriesName, data: values, color: undefined } as SeriesData],
          }
        }
      }
    }

    // --- Fallback: index-based labels ---
    if (!dataSource && propLabels && propLabels.length > 0) {
      return { chartLabels: propLabels, alignedSeries: normalizedSeries }
    }

    const maxDataLength = normalizedSeries.map(s => s.data?.length ?? 0).reduce((a, b) => Math.max(a, b), 0)
    return {
      chartLabels: Array.from({ length: maxDataLength }, (_, i) => `${i}`),
      alignedSeries: normalizedSeries,
    }
  }, [data, sources, dataMapping, normalizedSeries, dataSource, propLabels, getSeriesName])

  const series = alignedSeries.length > 0 ? alignedSeries : normalizedSeries

  // Build chart data for recharts — ensure all values are numeric
  const chartData = useMemo(() => {
    return chartLabels.map((label, idx) => {
      const point: any = { name: label }
      series.forEach((s, i) => {
        const raw = s.data?.[idx]
        point[`series${i}`] = raw != null ? extractNumericValue(raw) : null
      })
      return point
    })
  }, [chartLabels, series])

  // Loading state
  if (showLoading) {
    return (
      <div className={cn(dashboardCardBase, 'h-full flex flex-col', config.padding, className)}>
        {title && (
          <div className={cn('mb-3', indicatorFontWeight.title, config.titleText)}>{title}</div>
        )}
        <Skeleton className={cn('w-full', 'flex-1 min-h-0')} />
      </div>
    )
  }

  // Error state
  if (error && dataSource) {
    return <ErrorState size={size} className={className} />
  }

  // Empty state - only when dataSource is configured, data finished loading, and no data at all
  // Note: if multi-source all returned null, we already fell through to fallback data above,
  // so series will have sample data. Only show empty state when we have a dataSource but
  // the single-source case produced nothing.
  if (dataSource && !loading && series.length === 0) {
    return <EmptyState size={size} className={className} message={title ? `${title} - No Data Available` : undefined} />
  }

  if (!loading && chartData.length === 0) {
    return <EmptyState size={size} className={className} message={title ? `${title} - No Data Available` : undefined} />
  }

  return (
    <div className={cn(dashboardCardBase, config.padding, className)}>
      {title && (
        <div className={cn('mb-3', indicatorFontWeight.title, config.titleText)}>{title}</div>
      )}
      <ChartContainer>
        <LineChartWithDimensions data={chartData} series={series} showGrid={showGrid} showTooltip={showTooltip} showLegend={showLegend} color={color} smooth={smooth} fillArea={fillArea} />
      </ChartContainer>
    </div>
  )
}

export const LineChart = memo(LineChartInner)

// Memoized Recharts renderers — skip re-render when staggered data hasn't changed.
// This is critical because Recharts doesn't use React.memo internally, so every
// parent re-render causes a full SVG re-render even with identical data.

const LineChartRenderer = createMemoRenderer(({ data, series, showGrid, showTooltip, showLegend, color, smooth, fillArea, width, height, uid }: {
  data: any[]
  series: SeriesData[]
  showGrid: boolean
  showTooltip: boolean
  showLegend: boolean
  color?: string
  smooth: boolean
  fillArea: boolean
  width: number
  height: number
  uid: string
}) => (
  <RechartsLineChart width={width} height={height} data={data} margin={{ top: 5, right: 5, bottom: 0, left: 0 }}>
    <defs>
      {series.map((s, i) => {
        const seriesColor = s.color || color || fallbackColors[i % fallbackColors.length]
        return (
          <linearGradient key={i} id={`line-grad-${uid}-${i}`} x1="0" y1="0" x2="0" y2="1">
            <stop offset="5%" stopColor={seriesColor} stopOpacity={0.2} />
            <stop offset="95%" stopColor={seriesColor} stopOpacity={0} />
          </linearGradient>
        )
      })}
    </defs>
    {showGrid && <CartesianGrid vertical={false} strokeDasharray="4 4" className="stroke-muted" />}
    <XAxis dataKey="name" axisLine={false} tickLine={false} tickMargin={10} tick={{ fill: 'var(--muted-foreground)', fontSize: 10 }} interval="preserveStartEnd" />
    <YAxis axisLine={false} tickLine={false} tickMargin={10} width={32} tick={{ fill: 'var(--muted-foreground)', fontSize: 10 }} />
    {showTooltip && <Tooltip content={<ChartTooltip />} />}
    {showLegend && <Legend />}
    {series.map((s, i) => {
      const seriesColor = s.color || color || fallbackColors[i % fallbackColors.length]
      return (
        <g key={i}>
          {fillArea && <Area type={smooth ? 'monotone' : 'linear'} dataKey={`series${i}`} stroke="none" fill={`url(#line-grad-${uid}-${i})`} isAnimationActive animationDuration={800} />}
          <Line type={smooth ? 'monotone' : 'linear'} dataKey={`series${i}`} name={s.name} stroke={seriesColor} strokeWidth={2} dot={false} isAnimationActive animationDuration={800} activeDot={{ r: 4, className: 'fill-background stroke-[2px]' }} strokeLinejoin="round" strokeLinecap="round" />
        </g>
      )
    })}
  </RechartsLineChart>
))

const AreaChartRenderer = createMemoRenderer(({ data, series, showGrid, showTooltip, showLegend, color, smooth, width, height, uid }: {
  data: any[]
  series: SeriesData[]
  showGrid: boolean
  showTooltip: boolean
  showLegend: boolean
  color?: string
  smooth: boolean
  width: number
  height: number
  uid: string
}) => (
  <RechartsAreaChart width={width} height={height} data={data} margin={{ top: 5, right: 5, bottom: 0, left: 0 }}>
    <defs>
      {series.map((s, i) => {
        const seriesColor = s.color || color || fallbackColors[i % fallbackColors.length]
        return (
          <linearGradient key={i} id={`area-grad-${uid}-${i}`} x1="0" y1="0" x2="0" y2="1">
            <stop offset="5%" stopColor={seriesColor} stopOpacity={0.3} />
            <stop offset="95%" stopColor={seriesColor} stopOpacity={0.02} />
          </linearGradient>
        )
      })}
    </defs>
    {showGrid && <CartesianGrid vertical={false} strokeDasharray="4 4" className="stroke-muted" />}
    <XAxis dataKey="name" axisLine={false} tickLine={false} tickMargin={10} tick={{ fill: 'var(--muted-foreground)', fontSize: 10 }} interval="preserveStartEnd" />
    <YAxis axisLine={false} tickLine={false} tickMargin={10} width={32} tick={{ fill: 'var(--muted-foreground)', fontSize: 10 }} />
    {showTooltip && <Tooltip content={<ChartTooltip />} />}
    {showLegend && <Legend />}
    {series.map((s, i) => {
      const seriesColor = s.color || color || fallbackColors[i % fallbackColors.length]
      return <Area key={i} type={smooth ? 'monotone' : 'linear'} dataKey={`series${i}`} name={s.name} stroke={seriesColor} strokeWidth={2} fill={`url(#area-grad-${uid}-${i})`} isAnimationActive animationDuration={800} strokeLinejoin="round" strokeLinecap="round" connectNulls />
    })}
  </RechartsAreaChart>
))

// Stable chart wrappers that use useChartDimensions instead of ResponsiveContainer
// to prevent blank frames during scroll

function LineChartWithDimensions({ data, series, showGrid, showTooltip, showLegend, color, smooth, fillArea }: {
  data: any[]
  series: SeriesData[]
  showGrid: boolean
  showTooltip: boolean
  showLegend: boolean
  color?: string
  smooth: boolean
  fillArea: boolean
}) {
  const { ref, width, height, turn } = useChartDimensions()
  const staggeredData = useStaggeredData(data, turn)
  const staggeredSeries = useStaggeredData(series, turn)
  const uid = useId().replace(/:/g, '')
  return (
    <div ref={ref} style={{ width: '100%', height: '100%' }}>
      {width > 0 && height > 0 && (
        <LineChartRenderer uid={uid} data={staggeredData} series={staggeredSeries} showGrid={showGrid} showTooltip={showTooltip} showLegend={showLegend} color={color} smooth={smooth} fillArea={fillArea} width={width} height={height} />
      )}
    </div>
  )
}

function AreaChartWithDimensions({ data, series, showGrid, showTooltip, showLegend, color, smooth }: {
  data: any[]
  series: SeriesData[]
  showGrid: boolean
  showTooltip: boolean
  showLegend: boolean
  color?: string
  smooth: boolean
}) {
  const { ref, width, height, turn } = useChartDimensions()
  const staggeredData = useStaggeredData(data, turn)
  const staggeredSeries = useStaggeredData(series, turn)
  const uid = useId().replace(/:/g, '')
  return (
    <div ref={ref} style={{ width: '100%', height: '100%' }}>
      {width > 0 && height > 0 && (
        <AreaChartRenderer uid={uid} data={staggeredData} series={staggeredSeries} showGrid={showGrid} showTooltip={showTooltip} showLegend={showLegend} color={color} smooth={smooth} width={width} height={height} />
      )}
    </div>
  )
}

/**
 * Area Chart Component
 *
 * Enhanced with time-series aggregation support:
 * - Aggregate multiple data points into single values
 * - Support for different aggregation methods (latest, avg, sum, etc.)
 * - Time window selection for data scope
 * - Chart view modes: timeseries, snapshot
 */

const DEFAULT_AREA_DATA: SeriesData[] = [{ name: 'Revenue', data: [12, 19, 15, 25, 22, 30, 28, 35, 32, 40, 38, 45] }]

export interface AreaChartProps {
  // Data source configuration
  dataSource?: DataSourceOrList  // Support both single and multiple data sources

  // Data
  series?: SeriesData[]
  labels?: string[]

  // Display options
  title?: string
  height?: number | 'auto'
  showGrid?: boolean
  showLegend?: boolean
  showTooltip?: boolean
  smooth?: boolean
  color?: string
  size?: 'sm' | 'md' | 'lg'

  // === Telemetry options ===
  // Legacy options (kept for backward compatibility)
  limit?: number
  timeRange?: number

  // === New: Data transformation options ===
  // How to aggregate time-series data
  aggregate?: TelemetryAggregate

  // Data mapping configuration
  dataMapping?: TimeSeriesMappingConfig

  className?: string
}

export const AreaChart = memo(function AreaChart({
  dataSource,
  series: propSeries,
  labels,
  title,
  showGrid = false,
  showLegend = false,
  showTooltip = true,
  smooth = true,
  color,
  size = 'md',
  limit = 50,
  timeRange = 1,
  aggregate = 'raw',  // Default to raw for area charts (show time series)
  dataMapping,
  className,
}: AreaChartProps) {
  const { t } = useTranslation('dashboardComponents')
  const config = dashboardComponentSize[size]

  // Shared data pipeline — same pattern as LineChart
  const {
    sources, data, loading, error,
    hasData, showLoading, getSeriesName,
  } = useChartPipeline<any>({
    dataSource,
    aggregate,
    limit,
    timeRange,
    fallback: propSeries ?? [],
    preserveMultiple: true,
  })

  // Transform data to series format — same pattern as LineChart
  const normalizedSeries: SeriesData[] = useMemo(() => {

    // Multi-source case - data should be array of arrays from useDataSource with preserveMultiple
    if (isMultiSourceData(data, sources.length)) {
      const seriesResult = sources.map((ds, idx) => {
        const sourceData = idx < data.length ? data[idx] : []
        let values: number[] = []
        if (Array.isArray(sourceData)) {
          if (isNumberArray(sourceData)) {
            values = sourceData as number[]
          } else {
            const { values: v } = transformTelemetryToChartData(sourceData, dataMapping)
            values = v
          }
        }

        const seriesName = getSeriesName(ds, idx)
        return {
          name: seriesName,
          data: values,
          color: undefined,
        } as SeriesData
      })

      // If ALL series have empty data, fall through to fallback/sample data
      const hasAnyData = seriesResult.some(s => s.data.length > 0)
      if (hasAnyData) {
        return seriesResult
      }
      // Fall through to use fallback or sample data below
    }

    // Handle telemetry raw data FIRST (when dataSource is provided)
    if (dataSource && isSeriesDataArray(data)) {
      return data
    }

    if (dataSource && Array.isArray(data) && data.length > 0 && !isSeriesDataArray(data)) {

      // Single source - transform telemetry points
      const { labels: telemetryLabels, values } = transformTelemetryToChartData(data, dataMapping)
      if (values.length > 0) {
        const singleSource = sources[0]
        const seriesName = singleSource ? getSeriesName(singleSource, 0) : 'Value'
        return [{ name: seriesName, data: values, color: undefined } as SeriesData]
      }
    }

    // Handle single number from data source
    if (dataSource && typeof data === 'number') {
      return [{ name: 'Value', data: [data], color: undefined } as SeriesData]
    }

    // Handle number array from data source
    if (dataSource && isNumberArray(data)) {
      return [{ name: 'Value', data: data as number[], color: undefined } as SeriesData]
    }

    // If no dataSource, use propSeries (static data) — filter out items without data
    if (!dataSource && propSeries && Array.isArray(propSeries) && propSeries.length > 0) {
      const validSeries = propSeries.filter(s => Array.isArray(s?.data))
      if (validSeries.length > 0) return validSeries
    }

    // When dataSource is configured but produced no data, show empty state (not sample data)
    if (dataSource) return []

    // Default fallback (no dataSource = preview mode)
    return DEFAULT_AREA_DATA
  }, [data, propSeries, dataSource, dataMapping, sources, getSeriesName])

  // Extract timestamp-aligned data for multi-source, or sorted labels for single-source
  const { chartLabels, alignedSeries } = useMemo(() => {
    // --- Multi-source with timestamps: align by timestamp ---
    if (sources.length > 1 && isMultiSourceData(data, sources.length)) {
      const aligned = alignMultiSource(data, sources, getSeriesName, dataMapping)
      if (aligned) return { chartLabels: aligned.chartLabels, alignedSeries: aligned.series }
    }

    // --- Single source with timestamps ---
    if (dataSource && Array.isArray(data) && data.length > 0) {
      const first = data[0]
      if (typeof first === 'object' && first !== null && ('timestamp' in first || 't' in first || 'time' in first)) {
        const { labels: telemetryLabels, values } = transformTelemetryToChartData(data, dataMapping)
        if (telemetryLabels.length > 0) {
          const singleSource = sources[0]
          const seriesName = singleSource ? getSeriesName(singleSource, 0) : 'Value'
          return {
            chartLabels: telemetryLabels,
            alignedSeries: [{ name: seriesName, data: values, color: undefined } as SeriesData],
          }
        }
      }
    }

    // --- Fallback: index-based labels ---
    if (!dataSource && labels && labels.length > 0) {
      return { chartLabels: labels, alignedSeries: normalizedSeries }
    }

    const maxDataLength = normalizedSeries.map(s => s.data?.length ?? 0).reduce((a, b) => Math.max(a, b), 0)
    return {
      chartLabels: Array.from({ length: maxDataLength }, (_, i) => `${i}`),
      alignedSeries: normalizedSeries,
    }
  }, [data, sources, dataMapping, normalizedSeries, dataSource, labels, getSeriesName])

  const series = alignedSeries.length > 0 ? alignedSeries : normalizedSeries

  // Build chart data for recharts — ensure all values are numeric
  const chartData = useMemo(() => {
    return chartLabels.map((label, idx) => {
      const point: any = { name: label }
      series.forEach((s, i) => {
        const raw = s.data?.[idx]
        point[`series${i}`] = raw != null ? extractNumericValue(raw) : null
      })
      return point
    })
  }, [chartLabels, series])

  // Loading state
  if (showLoading) {
    return (
      <div className={cn(dashboardCardBase, 'h-full flex flex-col', config.padding, className)}>
        {title && (
          <div className={cn('mb-3', indicatorFontWeight.title, config.titleText)}>{title}</div>
        )}
        <Skeleton className={cn('w-full', 'flex-1 min-h-0')} />
      </div>
    )
  }

  // Error state
  if (error && dataSource) {
    return <ErrorState size={size} className={className} />
  }

  // Empty state - only when dataSource is configured, data finished loading, and no data at all
  if (dataSource && !loading && series.length === 0) {
    return <EmptyState size={size} className={className} message={title ? `${title} - No Data Available` : undefined} />
  }

  if (!loading && chartData.length === 0) {
    return <EmptyState size={size} className={className} message={title ? `${title} - No Data Available` : undefined} />
  }

  return (
    <div className={cn(dashboardCardBase, config.padding, className)}>
      {title && (
        <div className={cn('mb-3', indicatorFontWeight.title, config.titleText)}>{title}</div>
      )}
      <ChartContainer>
        <AreaChartWithDimensions data={chartData} series={series} showGrid={showGrid} showTooltip={showTooltip} showLegend={showLegend} color={color} smooth={smooth} />
      </ChartContainer>
    </div>
  )
})
