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

import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import {
  BarChart as RechartsBarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
  ResponsiveContainer,
  Cell,
} from 'recharts'
import { Skeleton } from '@/components/ui/skeleton'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { cn } from '@/lib/utils'
import { DataMapper, type CategoricalMappingConfig } from '@/lib/dataMapping'
import { useDataSource } from '@/hooks/useDataSource'
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
import { indicatorFontWeight } from '@/design-system/tokens/indicator'
import { chartColors as designChartColors } from '@/design-system/tokens/color'
import type { DataSource, DataSourceOrList, TelemetryAggregate, ChartViewMode } from '@/types/dashboard'
import { normalizeDataSource } from '@/types/dashboard'
import { EmptyState } from '../shared'
import {
  getEffectiveAggregate,
  getEffectiveTimeWindow,
  timeWindowToHours,
  aggregateData,
  transformToBarData,
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
 * shadcn/ui style tooltip component
 */
function ChartTooltip({ active, payload }: { active?: boolean; payload?: any[] }) {
  if (!active || !payload?.length) return null

  return (
    <div className="rounded-lg border bg-background p-2 shadow-md">
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

/**
 * Convert device/metric source to telemetry for bar charts.
 * Bar charts can display time-series data as discrete bars.
 * Now supports the new timeWindow and aggregateExt options.
 */
function toTelemetrySource(
  dataSource?: DataSource,
  limit: number = 24,
  timeRange: number = 1
): DataSource | undefined {
  if (!dataSource) {
    return undefined
  }

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
      type: 'telemetry' as const,
      deviceId: dataSource.deviceId,
      metricId: dataSource.metricId ?? dataSource.property ?? 'value',
      timeRange: timeWindowToHours(effectiveTimeWindow.type),
      limit: limit,
      aggregate: effectiveAggregate === 'raw' ? 'raw' : 'avg',
      params: {
        includeRawPoints: true,
      },
      transform: 'raw' as const,
    }
  }

  // Convert metric type with deviceId to telemetry
  // Metric type without deviceId will be handled by useDataSource's dynamic lookup
  if (dataSource.type === 'metric' && dataSource.deviceId) {
    return {
      type: 'telemetry' as const,
      deviceId: dataSource.deviceId,
      metricId: dataSource.metricId ?? dataSource.property ?? 'value',
      timeRange: timeWindowToHours(effectiveTimeWindow.type),
      limit: limit,
      aggregate: effectiveAggregate === 'raw' ? 'raw' : 'avg',
      params: {
        includeRawPoints: true,
      },
      transform: 'raw' as const,
    }
  }

  return dataSource
}

/**
 * Transform raw telemetry points to chart data using DataMapper
 * For categorical data (strings) or when aggregate is 'count', groups by value and counts occurrences
 */
function transformTelemetryToBarData(
  data: unknown,
  dataMapping?: CategoricalMappingConfig,
  aggregate?: TelemetryAggregate
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
    let telemetryPoints = data as Array<{ timestamp?: number; value: unknown }>

    // Sort by timestamp ascending (oldest first) for proper time series display
    // Use explicit sort instead of reverse to handle any data order correctly
    if (telemetryPoints.length > 1) {
      telemetryPoints = [...telemetryPoints].sort((a, b) => {
        const at = a.timestamp ?? 0
        const bt = b.timestamp ?? 0
        return at - bt  // ascending: oldest first
      })
    }

    // Check if values are categorical (strings) OR if aggregate is 'count' - group and count for distribution
    const firstValue = telemetryPoints[0]?.value
    const isCategorical = typeof firstValue === 'string'
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

    // For numeric telemetry data without count aggregation, create time-based labels
    return telemetryPoints.map((point, index) => {
      let name = `${index + 1}`
      if (point.timestamp) {
        const date = new Date(point.timestamp > 10000000000 ? point.timestamp : point.timestamp * 1000)
        if (!isNaN(date.getTime())) {
          name = date.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit' })
        }
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
  stacked?: boolean

  // === Telemetry options ===
  // Legacy options (kept for backward compatibility)
  limit?: number
  timeRange?: number

  // === New: Data transformation options ===
  // How to aggregate time-series data
  aggregate?: TelemetryAggregate
  // Time window for data
  timeWindow?: 'now' | 'last_5min' | 'last_15min' | 'last_30min' | 'last_1hour' | 'last_6hours' | 'last_24hours'
  // Chart view mode - how to interpret data
  chartViewMode?: ChartViewMode

  // Data mapping configuration
  dataMapping?: CategoricalMappingConfig

  // Styling
  color?: string
  size?: 'sm' | 'md' | 'lg'
  className?: string
}

export function BarChart({
  dataSource,
  data: propData,
  title,
  height = 'auto',
  showGrid = false,
  showLegend = false,
  showTooltip = true,
  layout = 'vertical',
  color,
  size = 'md',
  limit = 24,
  timeRange = 1,
  aggregate = 'raw',  // Default to raw for bar charts (show time series)
  timeWindow,
  chartViewMode = 'timeseries',
  dataMapping,
  className,
}: BarChartProps) {
  const { t } = useTranslation('dashboardComponents')
  const config = dashboardComponentSize[size]

  // Get effective aggregate from dataSource or props
  const effectiveAggregate = useMemo(() => {
    const sources = normalizeDataSource(dataSource)
    if (sources.length > 0 && sources[0].aggregateExt) {
      return sources[0].aggregateExt
    }
    return aggregate
  }, [dataSource, aggregate])

  // Get effective chart view mode
  const effectiveViewMode = useMemo(() => {
    const sources = normalizeDataSource(dataSource)
    if (sources.length > 0 && sources[0].chartViewMode) {
      return sources[0].chartViewMode
    }
    return chartViewMode
  }, [dataSource, chartViewMode])

  // Normalize data sources for historical data
  const telemetrySources = useMemo(() => {
    const sources = normalizeDataSource(dataSource)
    return sources.map(ds => toTelemetrySource(ds, limit, timeRange)).filter((ds): ds is DataSource => ds !== undefined)
  }, [dataSource, limit, timeRange])

  const { data, loading } = useDataSource<BarData[] | number[] | number[][]>(
    telemetrySources.length > 0 ? (telemetrySources.length === 1 ? telemetrySources[0] : telemetrySources) : undefined,
    {
      fallback: undefined,
      preserveMultiple: true,
    }
  )

  // Get device names for series labels
  const getDeviceName = (deviceId?: string): string => {
    if (!deviceId) return t('chart.value')
    return deviceId.replace(/[-_]/g, ' ').replace(/\b\w/g, c => c.toUpperCase())
  }

  const getPropertyDisplayName = (property?: string): string => {
    if (!property) return t('chart.value')
    const propertyNames: Record<string, string> = {
      temperature: t('chart.temperature'),
      humidity: t('chart.humidity'),
      temp: t('chart.temperature'),
      value: t('chart.value'),
    }
    return propertyNames[property] || property.replace(/[-_]/g, ' ')
  }

  // Check if data is multi-source (array of arrays)
  const isMultiSource = (data: unknown): boolean => {
    return Array.isArray(data) && data.length > 0 && Array.isArray(data[0])
  }

  // For multi-series bar chart, transform data to recharts format
  const chartData = useMemo(() => {
    const sources = normalizeDataSource(dataSource)

    // Helper to extract numeric value from data point
    const extractNumericValue = (item: unknown): number => {
      if (typeof item === 'number') return item
      if (typeof item === 'object' && item !== null && 'value' in item) {
        const val = (item as { value: unknown }).value
        return typeof val === 'number' ? val : 0
      }
      return 0
    }

    // Multi-source data - create grouped bar chart
    // preserveMultiple returns array of arrays where length equals sources length
    if (sources.length > 1 && Array.isArray(data) && data.length === sources.length) {
      // Check if any source contains categorical data (string values)
      const hasCategoricalData = data.some((arr: unknown) => {
        if (Array.isArray(arr) && arr.length > 0) {
          const first = arr[0]
          return typeof first === 'object' && first !== null && 'value' in first && typeof first.value === 'string'
        }
        return false
      })
      const forceCounting = effectiveAggregate === 'count'

      // If categorical data or count aggregate, combine all sources and show distribution
      if (hasCategoricalData || forceCounting) {
        const combinedCounts = new Map<string, number>()

        // Process all data sources and count occurrences
        for (const arr of data as unknown[][]) {
          if (!Array.isArray(arr)) continue
          for (const item of arr) {
            if (typeof item === 'object' && item !== null && 'value' in item) {
              const key = String(item.value ?? 'unknown')
              combinedCounts.set(key, (combinedCounts.get(key) ?? 0) + 1)
            }
          }
        }

        // Convert to BarData format, sorted by count descending
        return Array.from(combinedCounts.entries())
          .sort((a, b) => b[1] - a[1])
          .map(([name, value]) => ({
            name,
            value,
          }))
      }

      // Numeric data - create grouped bar chart
      // Handle both number[][] and Array<{timestamp, value}> formats
      const sourceArrays = data as unknown[][]
      const maxLength = Math.max(...sourceArrays.map(arr => Array.isArray(arr) ? arr.length : 0))

      // Determine if we should use timestamps for labels
      const useTimestampLabels = sourceArrays.some(arr =>
        Array.isArray(arr) && arr.length > 0 &&
        typeof arr[0] === 'object' && arr[0] !== null && 'timestamp' in arr[0]
      )

      return Array.from({ length: maxLength }, (_, idx) => {
        const point: any = {}
        sources.forEach((ds, i) => {
          const sourceArray = sourceArrays[i]
          if (!Array.isArray(sourceArray)) {
            point[`series${i}`] = 0
            return
          }
          const item = sourceArray[idx]
          const arrValue = extractNumericValue(item)

          // Use timestamp-based labels if available
          if (idx === 0) {
            if (useTimestampLabels && typeof item === 'object' && item !== null && 'timestamp' in item) {
              const ts = (item as { timestamp: number }).timestamp
              const date = new Date(ts > 10000000000 ? ts : ts * 1000)
              point.name = !isNaN(date.getTime())
                ? date.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', second: '2-digit' })
                : `${idx + 1}`
            } else {
              point.name = `${idx + 1}`
            }
            // Store series names for legend
            point.seriesNames = sources.map((ds, si) => {
              return ds.deviceId
                ? `${getDeviceName(ds.deviceId)} · ${getPropertyDisplayName(ds.metricId || ds.property)}`
                : t('chart.series', { count: si + 1 })
            })
          }

          point[`series${i}`] = arrValue
        })
        return point
      })
    }

    // Single source - handle as before
    if (dataSource && Array.isArray(data) && data.length > 0) {
      const first = data[0]
      // Check if already in BarData format (has both 'name' AND 'value')
      if (typeof first === 'object' && first !== null && 'value' in first && 'name' in first) {
        return data as BarData[]
      }

      // Transform telemetry points (handles both numeric and categorical data)
      // Pass aggregate setting to influence transformation behavior
      const transformed = transformTelemetryToBarData(data, dataMapping, effectiveAggregate)
      if (transformed.length > 0) {
        return transformed
      }
    }

    // Handle number array from data source
    if (dataSource && Array.isArray(data) && data.length > 0 && typeof data[0] === 'number') {
      return (data as number[]).map((value, index) => ({
        name: `${index + 1}`,
        value,
      }))
    }

    // If no dataSource, use propData (static data)
    if (!dataSource && propData && Array.isArray(propData) && propData.length > 0) {
      return propData
    }

    // If dataSource is set but data is empty, return empty array (will show EmptyState)
    if (dataSource && !loading && Array.isArray(data) && data.length === 0) {
      return []
    }

    // Return default sample data for preview only (no dataSource)
    return [
      { name: t('chart.jan'), value: 12 },
      { name: t('chart.feb'), value: 18 },
      { name: t('chart.mar'), value: 15 },
      { name: t('chart.apr'), value: 22 },
      { name: t('chart.may'), value: 19 },
      { name: t('chart.jun'), value: 25 },
    ]
  }, [data, propData, dataSource, loading, dataMapping, effectiveAggregate])

  // Get series info for multi-source rendering
  const seriesInfo = useMemo(() => {
    const sources = normalizeDataSource(dataSource)
    if (sources.length > 1 && Array.isArray(data) && data.length === sources.length) {
      return sources.map((ds, i) => ({
        dataKey: `series${i}`,
        name: ds.deviceId
          ? `${getDeviceName(ds.deviceId)} · ${getPropertyDisplayName(ds.metricId || ds.property)}`
          : t('chart.series', { count: i + 1 }),
        color: color || fallbackColors[i % fallbackColors.length],
      }))
    }
    return null
  }, [dataSource, data, color, t])

  // Show loading skeleton when fetching data
  if (dataSource && loading) {
    return (
      <div className={cn(dashboardCardBase, config.padding, className)}>
        {title && (
          <div className={cn('mb-3', indicatorFontWeight.title, config.titleText)}>{title}</div>
        )}
        <Skeleton className={cn('w-full', size === 'sm' ? 'h-[120px]' : size === 'md' ? 'h-[180px]' : 'h-[240px]')} />
      </div>
    )
  }

  if (chartData.length === 0) {
    return <EmptyState size={size} className={className} message={title ? `${title} - No Data Available` : undefined} />
  }

  return (
    <div className={cn(dashboardCardBase, config.padding, className)}>
      {title && (
        <div className={cn('mb-3', indicatorFontWeight.title, config.titleText)}>{title}</div>
      )}
      <div className={cn('w-full', size === 'sm' ? 'h-[120px]' : size === 'md' ? 'h-[180px]' : 'h-[240px]')}>
        <ResponsiveContainer width="100%" height="100%">
          <RechartsBarChart
            data={chartData}
            margin={{ top: 5, right: 5, bottom: 0, left: 0 }}
            accessibilityLayer
          >
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

            {/* Multi-series bars */}
            {seriesInfo ? (
              seriesInfo.map((info) => (
                <Bar
                  key={info.dataKey}
                  dataKey={info.dataKey}
                  name={info.name}
                  fill={info.color}
                  radius={4}
                />
              ))
            ) : (
              /* Single series bar */
              <Bar
                dataKey="value"
                fill={color || fallbackColors[0]}
                radius={4}
              >
                {/* Only use different colors per bar for categorical/distribution data */}
                {chartData.some(d => d.color) && chartData.map((entry, index) => (
                  <Cell
                    key={`cell-${index}`}
                    fill={entry.color || color || fallbackColors[index % fallbackColors.length]}
                  />
                ))}
              </Bar>
            )}
          </RechartsBarChart>
        </ResponsiveContainer>
      </div>
    </div>
  )
}
