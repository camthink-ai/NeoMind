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

import { useMemo, useCallback } from 'react'
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
  ResponsiveContainer,
} from 'recharts'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'
import { DataMapper, type TimeSeriesMappingConfig } from '@/lib/dataMapping'
import { useDataSource } from '@/hooks/useDataSource'
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
import { indicatorFontWeight } from '@/design-system/tokens/indicator'
import { chartColors as designChartColors } from '@/design-system/tokens/color'
import type { DataSource, DataSourceOrList, TelemetryAggregate, ChartViewMode } from '@/types/dashboard'
import { normalizeDataSource } from '@/types/dashboard'
import { EmptyState, ErrorState } from '../shared'
import {
  getEffectiveAggregate,
  getEffectiveTimeWindow,
  timeWindowToHours,
  type TimeSeriesData,
} from '@/lib/telemetryTransform'

// Use design system chart colors
const chartColors = designChartColors

// Fallback colors as hex values for SVG
const fallbackColors = [
  '#8b5cf6', // Purple
  '#22c55e', // Green
  '#f59e0b', // Yellow
  '#f97316', // Orange
  '#ec4899', // Pink
]

/**
 * Convert device/metric source to telemetry for line charts.
 * Line charts display time-series data as connected points.
 * Now supports the new timeWindow and aggregateExt options.
 */
function toTelemetrySource(
  dataSource?: DataSource,
  limit: number = 50,
  timeRange: number = 1
): DataSource | undefined {
  if (!dataSource) return undefined

  // Get effective time window (new or legacy)
  const effectiveTimeWindow = getEffectiveTimeWindow(dataSource)
  const effectiveAggregate = getEffectiveAggregate(dataSource)

  // If already telemetry type, update with settings
  if (dataSource.type === 'telemetry') {
    return {
      ...dataSource,
      limit: dataSource.limit ?? limit,
      timeRange: dataSource.timeRange ?? timeWindowToHours(effectiveTimeWindow.type),
      aggregate: dataSource.aggregate ?? (effectiveAggregate === 'raw' ? 'raw' : 'avg'),
      params: {
        ...dataSource.params,
        includeRawPoints: true,
      },
      transform: 'raw',
    }
  }

  // Convert device type to telemetry for historical data
  if (dataSource.type === 'device' && dataSource.deviceId) {
    return {
      type: 'telemetry',
      deviceId: dataSource.deviceId,
      metricId: dataSource.metricId ?? dataSource.property ?? 'value',
      timeRange: timeWindowToHours(effectiveTimeWindow.type),
      limit: limit,
      aggregate: effectiveAggregate === 'raw' ? 'raw' : 'avg',
      params: {
        includeRawPoints: true,
      },
      transform: 'raw',
    }
  }

  // Convert metric type with deviceId to telemetry
  // Metric type without deviceId will be handled by useDataSource's dynamic lookup
  if (dataSource.type === 'metric' && dataSource.deviceId) {
    return {
      type: 'telemetry',
      deviceId: dataSource.deviceId,
      metricId: dataSource.metricId ?? dataSource.property ?? 'value',
      timeRange: timeWindowToHours(effectiveTimeWindow.type),
      limit: limit,
      aggregate: effectiveAggregate === 'raw' ? 'raw' : 'avg',
      params: {
        includeRawPoints: true,
      },
      transform: 'raw',
    }
  }

  return dataSource
}

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
    let timeSeriesPoints = DataMapper.mapToTimeSeries(data, dataMapping)

    // Sort by timestamp ascending (oldest first) for proper time series display
    // Use explicit sort instead of reverse to handle any data order correctly
    if (timeSeriesPoints.length > 1) {
      timeSeriesPoints = [...timeSeriesPoints].sort((a, b) => {
        const at = a.timestamp ?? 0
        const bt = b.timestamp ?? 0
        return at - bt  // ascending: oldest first
      })
    }

    // Extract values and format labels from timestamps
    const values = timeSeriesPoints.map(p => p.value)
    const labels = timeSeriesPoints.map(p => {
      if (p.timestamp) {
        const date = new Date(p.timestamp > 10000000000 ? p.timestamp : p.timestamp * 1000)
        if (!isNaN(date.getTime())) {
          return date.toLocaleTimeString('en-US', {
            hour: '2-digit',
            minute: '2-digit',
            second: '2-digit',
          })
        }
      }
      return p.label || `${timeSeriesPoints.indexOf(p) + 1}`
    })

    return { labels, values }
  }

  return { labels: [], values: [] }
}

/**
 * Format timestamp to readable time
 */
function formatTimestamp(timestamp: string | number | undefined): string {
  if (!timestamp) return ''

  const date = new Date(typeof timestamp === 'number' ? timestamp * 1000 : timestamp)
  if (isNaN(date.getTime())) return String(timestamp)

  return date.toLocaleTimeString('en-US', {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })
}

export interface SeriesData {
  name: string
  data: number[]
  color?: string
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
  // Chart view mode - how to interpret data
  chartViewMode?: ChartViewMode

  // Data mapping configuration
  dataMapping?: TimeSeriesMappingConfig

  className?: string
}

function ChartTooltip({ active, payload, label }: { active?: boolean; payload?: any[]; label?: string }) {
  if (!active || !payload?.length) return null

  return (
    <div className="rounded-lg border bg-background p-2 shadow-md">
      {label && <div className="mb-1 text-xs text-muted-foreground">{label}</div>}
      <div className="grid gap-1.5 text-xs">
        {payload.map((entry: any, index: number) => (
          <div key={index} className="flex items-center gap-2">
            <div
              className="h-2 w-2 shrink-0 rounded-[2px]"
              style={{ backgroundColor: entry.color }}
            />
            <span className="text-muted-foreground font-medium">{entry.name}:</span>
            <span className="tabular-nums font-semibold">{entry.value}</span>
          </div>
        ))}
      </div>
    </div>
  )
}

export function LineChart({
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
  chartViewMode = 'timeseries',
  dataMapping,
  className,
}: LineChartProps) {
  const { t } = useTranslation('dashboardComponents')
  const config = dashboardComponentSize[size]

  // Normalize data sources once - reuse across all memoized calculations
  // This prevents repeated normalizeDataSource calls
  const sources = useMemo(() => normalizeDataSource(dataSource), [dataSource])

  // Get effective aggregate from dataSource or props
  const effectiveAggregate = useMemo(() => {
    if (sources.length > 0 && sources[0].aggregateExt) {
      return sources[0].aggregateExt
    }
    return aggregate
  }, [sources, aggregate])

  // Get effective chart view mode
  const effectiveViewMode = useMemo(() => {
    if (sources.length > 0 && sources[0].chartViewMode) {
      return sources[0].chartViewMode
    }
    return chartViewMode
  }, [sources, chartViewMode])

  // Normalize data sources for historical data
  // Convert single DataSource or DataSource[] to array of telemetry sources
  const telemetrySources = useMemo(() => {
    return sources.map(ds => toTelemetrySource(ds, limit, timeRange)).filter((ds): ds is DataSource => ds !== undefined)
  }, [sources, limit, timeRange])

  const { data, loading, error } = useDataSource<any>(
    telemetrySources.length > 0 ? (telemetrySources.length === 1 ? telemetrySources[0] : telemetrySources) : undefined,
    {
      fallback: propSeries ?? [],
      preserveMultiple: true,  // Keep multiple data sources separate
    }
  )

  // Get device names for series labels
  const getDeviceName = useCallback((deviceId?: string): string => {
    if (!deviceId) return t('chart.value')
    return deviceId.replace(/[-_]/g, ' ').replace(/\b\w/g, c => c.toUpperCase())
  }, [t])

  // Get property name for series labels
  const getPropertyDisplayName = useCallback((property?: string): string => {
    if (!property) return t('chart.value')
    const propertyNames: Record<string, string> = {
      temperature: t('chart.temperature'),
      humidity: t('chart.humidity'),
      temp: t('chart.temperature'),
      value: t('chart.value'),
    }
    return propertyNames[property] || property.replace(/[-_]/g, ' ')
  }, [t])

  // Transform data to series format
  const normalizedSeries: SeriesData[] = useMemo(() => {

    // Multi-source case - data should be array of arrays from useDataSource with preserveMultiple
    if (sources.length > 1 && Array.isArray(data) && data.length === sources.length) {
      return sources.map((ds, idx) => {
        const sourceData = data[idx]
        // Transform telemetry points for this source
        let values: number[] = []
        if (Array.isArray(sourceData)) {
          if (typeof sourceData[0] === 'number') {
            values = sourceData as number[]
          } else {
            // Transform telemetry points - sourceData is raw telemetry points with timestamps
            const { values: v } = transformTelemetryToChartData(sourceData, dataMapping)
            values = v
          }
        }

        const seriesName = ds.deviceId
          ? `${getDeviceName(ds.deviceId)} · ${getPropertyDisplayName(ds.metricId || ds.property)}`
          : t('chart.series', { count: idx + 1 })
        return {
          name: seriesName,
          data: values,
          color: undefined,
        } as SeriesData
      })
    }

    // Handle telemetry raw data FIRST (when dataSource is provided)
    if (dataSource && Array.isArray(data) && data.length > 0) {
      // Check if it's already SeriesData format
      const first = data[0]
      if (typeof first === 'object' && first !== null && 'data' in first && Array.isArray(first.data)) {
        return data as SeriesData[]
      }

      // Single source - transform telemetry points
      const { labels, values } = transformTelemetryToChartData(data, dataMapping)
      if (values.length > 0) {
        const singleSource = sources[0]
        const seriesName = singleSource?.deviceId
          ? `${getDeviceName(singleSource.deviceId)} · ${getPropertyDisplayName(singleSource.metricId || singleSource.property)}`
          : 'Value'
        return [{ name: seriesName, data: values, color: undefined } as SeriesData]
      }
    }

    // Handle single number from data source
    if (dataSource && typeof data === 'number') {
      return [{ name: 'Value', data: [data], color: undefined } as SeriesData]
    }

    // Handle number array from data source
    if (dataSource && Array.isArray(data) && data.length > 0 && typeof data[0] === 'number') {
      return [{ name: 'Value', data: data as number[], color: undefined } as SeriesData]
    }

    // If no dataSource, use propSeries (static data)
    if (!dataSource && propSeries && Array.isArray(propSeries) && propSeries.length > 0 && propSeries[0]?.data) {
      return propSeries
    }

    // Default fallback
    return [{
      name: 'Sample',
      data: [10, 15, 12, 18, 14, 20, 16, 22, 19, 25],
      color: undefined,
    } as SeriesData]
  }, [data, propSeries, dataSource, dataMapping])

  // Extract raw labels from telemetry data before any transformation
  // This must be computed before normalizedSeries to access raw timestamps
  const rawChartLabels = useMemo(() => {
    // Multi-source case - extract labels from first series raw telemetry data
    if (sources.length > 1 && Array.isArray(data) && data.length > 0) {
      const firstSeriesData = data[0]
      if (Array.isArray(firstSeriesData) && firstSeriesData.length > 0) {
        const first = firstSeriesData[0]
        // Check if it's raw telemetry points (has timestamp field)
        if (typeof first === 'object' && first !== null && ('timestamp' in first || 't' in first || 'time' in first)) {
          return (firstSeriesData as any[]).map(item => {
            const ts = item.timestamp ?? item.t ?? item.time
            if (typeof ts === 'number') {
              const date = new Date(ts * 1000)
              return date.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', second: '2-digit' })
            }
            return String(ts ?? '')
          })
        }
      }
    }

    // Single source case - extract labels from telemetry data
    if (dataSource && Array.isArray(data) && data.length > 0) {
      const first = data[0]
      // Check if it's raw telemetry points
      if (typeof first === 'object' && first !== null && ('timestamp' in first || 't' in first || 'time' in first)) {
        const { labels: telemetryLabels } = transformTelemetryToChartData(data, dataMapping)
        if (telemetryLabels.length > 0) {
          return telemetryLabels
        }
      }
    }

    return null // Signal that we couldn't extract raw labels
  }, [data, sources, dataMapping])

  // Generate labels from telemetry or use provided labels
  const chartLabels = useMemo(() => {
    // If we extracted raw labels from telemetry, use them
    if (rawChartLabels && rawChartLabels.length > 0) {
      return rawChartLabels
    }

    // If no dataSource, use propLabels (static labels)
    if (!dataSource && propLabels && propLabels.length > 0) {
      return propLabels
    }

    // Default indexed labels based on the longest series
    const maxDataLength = Math.max(...normalizedSeries.map(s => s.data.length), 0)
    return Array.from({ length: maxDataLength }, (_, i) => `${i}`)
  }, [rawChartLabels, propLabels, normalizedSeries, dataSource])

  const series = normalizedSeries

  // Build chart data for recharts
  const chartData = useMemo(() => {
    return chartLabels.map((label, idx) => {
      const point: any = { name: label }
      series.forEach((s, i) => {
        point[`series${i}`] = s.data[idx] ?? null
      })
      return point
    })
  }, [chartLabels, series])

  // Loading state
  if (loading) {
    return (
      <div className={cn(dashboardCardBase, config.padding, className)}>
        {title && (
          <div className={cn('mb-3', indicatorFontWeight.title, config.titleText)}>{title}</div>
        )}
        <Skeleton className={cn('w-full', size === 'sm' ? 'h-[120px]' : size === 'md' ? 'h-[180px]' : 'h-[240px]')} />
      </div>
    )
  }

  // Error state
  if (error && dataSource) {
    return <ErrorState size={size} className={className} />
  }

  // Empty state - only when dataSource is configured but no data available
  if (dataSource && (series.length === 0 || chartData.length === 0)) {
    return <EmptyState size={size} className={className} message={title ? `${title} - No Data Available` : undefined} />
  }

  return (
    <div className={cn(dashboardCardBase, config.padding, className)}>
      {title && (
        <div className={cn('mb-3', indicatorFontWeight.title, config.titleText)}>{title}</div>
      )}
      <div className={cn('w-full', size === 'sm' ? 'h-[120px]' : size === 'md' ? 'h-[180px]' : 'h-[240px]')}>
        <ResponsiveContainer width="100%" height="100%">
          <RechartsLineChart data={chartData} margin={{ top: 5, right: 5, bottom: 0, left: 0 }} accessibilityLayer>
            <defs>
              {series.map((s, i) => {
                const seriesColor = s.color || color || fallbackColors[i % fallbackColors.length]
                return (
                  <linearGradient key={i} id={`gradient-${i}`} x1="0" y1="0" x2="0" y2="1">
                    <stop offset="5%" stopColor={seriesColor} stopOpacity={0.2} />
                    <stop offset="95%" stopColor={seriesColor} stopOpacity={0} />
                  </linearGradient>
                )
              })}
            </defs>
            {showGrid && <CartesianGrid vertical={false} strokeDasharray="4 4" className="stroke-muted" />}
            <XAxis
              dataKey="name"
              axisLine={false}
              tickLine={false}
              tickMargin={10}
              tick={{ fill: 'hsl(var(--muted-foreground))', fontSize: 10 }}
              interval="preserveStartEnd"
            />
            <YAxis
              axisLine={false}
              tickLine={false}
              tickMargin={10}
              width={32}
              tick={{ fill: 'hsl(var(--muted-foreground))', fontSize: 10 }}
            />
            {showTooltip && <Tooltip content={<ChartTooltip />} />}
            {showLegend && <Legend />}
            {series.map((s, i) => {
              const seriesColor = s.color || color || fallbackColors[i % fallbackColors.length]
              return (
                <g key={i}>
                  {fillArea && (
                    <Area
                      type={smooth ? 'monotone' : 'linear'}
                      dataKey={`series${i}`}
                      stroke="none"
                      fill={`url(#gradient-${i})`}
                    />
                  )}
                  <Line
                    type={smooth ? 'monotone' : 'linear'}
                    dataKey={`series${i}`}
                    name={s.name}
                    stroke={seriesColor}
                    strokeWidth={2}
                    dot={false}
                    activeDot={{ r: 4, className: 'fill-background stroke-[2px]' }}
                    strokeLinejoin="round"
                    strokeLinecap="round"
                  />
                </g>
              )
            })}
          </RechartsLineChart>
        </ResponsiveContainer>
      </div>
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
  // Chart view mode - how to interpret data
  chartViewMode?: ChartViewMode

  // Data mapping configuration
  dataMapping?: TimeSeriesMappingConfig

  className?: string
}

export function AreaChart({
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
  chartViewMode = 'timeseries',
  dataMapping,
  className,
}: AreaChartProps) {
  const { t } = useTranslation('dashboardComponents')
  const config = dashboardComponentSize[size]
  const effectiveSeries = propSeries || DEFAULT_AREA_DATA

  // Normalize data sources once - reuse across all memoized calculations
  // This prevents repeated normalizeDataSource calls
  const sources = useMemo(() => normalizeDataSource(dataSource), [dataSource])

  // Get effective aggregate from dataSource or props
  const effectiveAggregate = useMemo(() => {
    if (sources.length > 0 && sources[0].aggregateExt) {
      return sources[0].aggregateExt
    }
    return aggregate
  }, [sources, aggregate])

  // Get effective chart view mode
  const effectiveViewMode = useMemo(() => {
    if (sources.length > 0 && sources[0].chartViewMode) {
      return sources[0].chartViewMode
    }
    return chartViewMode
  }, [sources, chartViewMode])

  // Normalize data sources for historical data
  const telemetrySources = useMemo(() => {
    return sources.map(ds => toTelemetrySource(ds, limit, timeRange)).filter((ds): ds is DataSource => ds !== undefined)
  }, [sources, limit, timeRange])

  const shouldFetch = telemetrySources.length > 0
  const { data: sourceData, loading, error } = useDataSource<SeriesData[] | number | number[]>(
    shouldFetch ? (telemetrySources.length === 1 ? telemetrySources[0] : telemetrySources) : undefined,
    shouldFetch ? { fallback: effectiveSeries, preserveMultiple: true } : undefined
  )
  const rawData = shouldFetch ? sourceData : effectiveSeries

  // Get device names for series labels
  const getDeviceName = useCallback((deviceId?: string): string => {
    if (!deviceId) return t('chart.value')
    return deviceId.replace(/[-_]/g, ' ').replace(/\b\w/g, c => c.toUpperCase())
  }, [t])

  const getPropertyDisplayName = useCallback((property?: string): string => {
    if (!property) return t('chart.value')
    const propertyNames: Record<string, string> = {
      temperature: t('chart.temperature'),
      humidity: t('chart.humidity'),
      temp: t('chart.temperature'),
      value: t('chart.value'),
    }
    return propertyNames[property] || property.replace(/[-_]/g, ' ')
  }, [t])

  const normalizedSeries: SeriesData[] = useMemo(() => {
    // Multi-source case - data should be array of arrays from useDataSource with preserveMultiple
    if (sources.length > 1 && Array.isArray(rawData) && rawData.length === sources.length) {
      return sources.map((ds, idx) => {
        const sourceData = rawData[idx]
        // Transform telemetry points for this source
        let values: number[] = []
        if (Array.isArray(sourceData)) {
          if (typeof sourceData[0] === 'number') {
            values = sourceData as number[]
          } else {
            // Transform telemetry points
            const { values: v } = transformTelemetryToChartData(sourceData, dataMapping)
            values = v
          }
        }

        const seriesName = ds.deviceId
          ? `${getDeviceName(ds.deviceId)} · ${getPropertyDisplayName(ds.property)}`
          : t('chart.series', { count: idx + 1 })
        return {
          name: seriesName,
          data: values,
          color: undefined,
        } as SeriesData
      })
    }

    // Handle telemetry data FIRST (when dataSource is provided)
    if (dataSource && Array.isArray(rawData) && rawData.length > 0) {
      const first = rawData[0]
      if (typeof first === 'object' && first !== null && 'data' in first && Array.isArray(first.data)) {
        return rawData as SeriesData[]
      }

      // Transform telemetry points
      const { values } = transformTelemetryToChartData(rawData, dataMapping)
      if (values.length > 0) {
        const seriesName = sources[0]?.deviceId
          ? `${getDeviceName(sources[0].deviceId)} · ${getPropertyDisplayName(sources[0].property)}`
          : 'Value'
        return [{ name: seriesName, data: values, color: undefined } as SeriesData]
      }
    }

    if (dataSource && typeof rawData === 'number') {
      return [{ name: 'Value', data: [rawData], color: undefined } as SeriesData]
    }

    if (dataSource && Array.isArray(rawData) && rawData.length > 0 && typeof rawData[0] === 'number') {
      return [{ name: 'Value', data: rawData as number[], color: undefined } as SeriesData]
    }

    // If no dataSource, use propSeries (static data)
    if (!dataSource && propSeries && Array.isArray(propSeries) && propSeries.length > 0 && propSeries[0]?.data) {
      return propSeries
    }

    return DEFAULT_AREA_DATA
  }, [rawData, propSeries, sources, dataMapping, getDeviceName, getPropertyDisplayName])

  const series = normalizedSeries

  // Extract raw labels from telemetry data before any transformation
  const rawChartLabels = useMemo(() => {
    // Multi-source case - extract labels from first series raw telemetry data
    if (sources.length > 1 && Array.isArray(rawData) && rawData.length > 0) {
      const firstSeriesData = rawData[0]
      if (Array.isArray(firstSeriesData) && firstSeriesData.length > 0) {
        const first = firstSeriesData[0]
        // Check if it's raw telemetry points (has timestamp field)
        if (typeof first === 'object' && first !== null && ('timestamp' in first || 't' in first || 'time' in first)) {
          return (firstSeriesData as any[]).map(item => {
            const ts = item.timestamp ?? item.t ?? item.time
            if (typeof ts === 'number') {
              const date = new Date(ts * 1000)
              return date.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', second: '2-digit' })
            }
            return String(ts ?? '')
          })
        }
      }
    }

    // Single source case - extract labels from telemetry data
    if (dataSource && Array.isArray(rawData) && rawData.length > 0) {
      const first = rawData[0]
      // Check if it's raw telemetry points
      if (typeof first === 'object' && first !== null && ('timestamp' in first || 't' in first || 'time' in first)) {
        const { labels: telemetryLabels } = transformTelemetryToChartData(rawData, dataMapping)
        if (telemetryLabels.length > 0) {
          return telemetryLabels
        }
      }
    }

    return null // Signal that we couldn't extract raw labels
  }, [rawData, sources, dataMapping])

  // Generate labels
  const chartLabels = useMemo(() => {
    // If we extracted raw labels from telemetry, use them
    if (rawChartLabels && rawChartLabels.length > 0) {
      return rawChartLabels
    }

    // If no dataSource, use propLabels (static labels)
    if (!dataSource && labels && labels.length > 0) {
      return labels
    }

    // Default indexed labels based on the longest series
    const maxDataLength = Math.max(...series.map(s => s.data.length), 0)
    return Array.from({ length: maxDataLength }, (_, i) => `${i}`)
  }, [rawChartLabels, labels, series, dataSource])

  const chartData = useMemo(() => {
    return chartLabels.map((label, idx) => {
      const point: any = { name: label }
      series.forEach((s, i) => {
        point[`series${i}`] = s.data[idx] ?? null
      })
      return point
    })
  }, [chartLabels, series])

  // Loading state
  if (loading) {
    return (
      <div className={cn(dashboardCardBase, config.padding, className)}>
        {title && (
          <div className={cn('mb-3', indicatorFontWeight.title, config.titleText)}>{title}</div>
        )}
        <Skeleton className={cn('w-full', size === 'sm' ? 'h-[120px]' : size === 'md' ? 'h-[180px]' : 'h-[240px]')} />
      </div>
    )
  }

  // Error state
  if (error && dataSource) {
    return <ErrorState size={size} className={className} />
  }

  // Empty state - only when dataSource is configured but no data available
  if (dataSource && (series.length === 0 || chartData.length === 0)) {
    return <EmptyState size={size} className={className} message={title ? `${title} - No Data Available` : undefined} />
  }

  return (
    <div className={cn(dashboardCardBase, config.padding, className)}>
      {title && (
        <div className={cn('mb-3', indicatorFontWeight.title, config.titleText)}>{title}</div>
      )}
      <div className={cn('w-full', size === 'sm' ? 'h-[120px]' : size === 'md' ? 'h-[180px]' : 'h-[240px]')}>
        <ResponsiveContainer width="100%" height="100%">
          <RechartsAreaChart data={chartData} margin={{ top: 5, right: 5, bottom: 0, left: 0 }} accessibilityLayer>
            {showGrid && <CartesianGrid vertical={false} strokeDasharray="4 4" className="stroke-muted" />}
            <XAxis
              dataKey="name"
              axisLine={false}
              tickLine={false}
              tickMargin={10}
              tick={{ fill: 'hsl(var(--muted-foreground))', fontSize: 10 }}
              interval="preserveStartEnd"
            />
            <YAxis
              axisLine={false}
              tickLine={false}
              tickMargin={10}
              width={32}
              tick={{ fill: 'hsl(var(--muted-foreground))', fontSize: 10 }}
            />
            {showTooltip && <Tooltip content={<ChartTooltip />} />}
            {showLegend && <Legend />}
            {series.map((s, i) => {
              const seriesColor = s.color || color || fallbackColors[i % fallbackColors.length]
              return (
                <Area
                  key={i}
                  type={smooth ? 'monotone' : 'linear'}
                  dataKey={`series${i}`}
                  name={s.name}
                  stroke={seriesColor}
                  strokeWidth={2}
                  fill={seriesColor}
                  fillOpacity={0.3}
                />
              )
            })}
          </RechartsAreaChart>
        </ResponsiveContainer>
      </div>
    </div>
  )
}
