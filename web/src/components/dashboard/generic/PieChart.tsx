/**
 * Pie Chart Component
 *
 * Unified with dashboard design system.
 * Supports telemetry data binding for categorical/part-to-whole data.
 *
 * Enhanced with time-series aggregation support:
 * - Aggregate multiple data points into single values
 * - Support for different aggregation methods (latest, avg, sum, etc.)
 * - Time window selection for data scope
 */

import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import {
  PieChart as RechartsPieChart,
  Pie,
  Cell,
  ResponsiveContainer,
  Tooltip,
  Legend,
} from 'recharts'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'
import { DataMapper, type CategoricalMappingConfig } from '@/lib/dataMapping'
import { useDataSource } from '@/hooks/useDataSource'
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
import { indicatorFontWeight } from '@/design-system/tokens/indicator'
import { chartColors as designChartColors } from '@/design-system/tokens/color'
import type { DataSource, DataSourceOrList, TelemetryAggregate } from '@/types/dashboard'
import { normalizeDataSource } from '@/types/dashboard'
import { EmptyState } from '../shared'
import {
  getEffectiveAggregate,
  getEffectiveTimeWindow,
  timeWindowToHours,
  parseTelemetryResponse,
  aggregateData,
  transformToPieData,
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
  '#06b6d4', // Cyan
]

/**
 * Convert device/metric source to telemetry for pie chart data.
 * For pie charts, we typically want the latest snapshot or aggregated categories.
 * Now supports the new timeWindow and aggregateExt options.
 */
function toTelemetrySource(
  dataSource?: DataSource,
  limit: number = 10,
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
      transform: dataSource.transform ?? 'raw',
    }
  }

  // Convert device type to telemetry for historical data
  // Note: metric type without deviceId should NOT be converted as it won't match events
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

  return dataSource
}

/**
 * Transform telemetry points to pie chart data using DataMapper.
 * Handles: [{ name, value }, { label, val }, { category, count }, { timestamp, value }] or raw numbers
 *
 * For categorical data (strings), groups by value and counts occurrences.
 * The aggregate parameter controls behavior:
 * - 'count': Always group by value and count (for distribution charts)
 * - Other: Auto-detect categorical vs numeric data
 */
function transformTelemetryToPieData(
  data: unknown,
  dataMapping?: CategoricalMappingConfig,
  aggregate?: TelemetryAggregate,
  t?: (key: string, params?: Record<string, unknown>) => string
): PieData[] {
  if (!data || !Array.isArray(data)) return []

  // Handle simple number array
  if (data.length > 0 && typeof data[0] === 'number') {
    return (data as number[]).map((value, index) => ({
      name: t ? t('chart.item', { count: index + 1 }) : `Item ${index + 1}`,
      value,
    }))
  }

  // Check if already in PieData format FIRST (has both name and value)
  if (data.length > 0 && typeof data[0] === 'object' && data[0] !== null && 'value' in data[0] && 'name' in data[0]) {
    return data as PieData[]
  }

  // Handle raw telemetry points format: [{ timestamp, value }, ...]
  // Only has 'value' but not 'name' → this is telemetry data
  // For pie chart, we aggregate or show distribution over time
  if (data.length > 0 && typeof data[0] === 'object' && data[0] !== null && 'value' in data[0]) {
    const telemetryPoints = data as Array<{ timestamp?: number; value: unknown }>

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

      // Convert to PieData format, sorted by count descending
      return Array.from(counts.entries())
        .sort((a, b) => b[1] - a[1])
        .map(([name, value]) => ({
          name,
          value,
        }))
    }

    // For numeric telemetry data without count aggregation, create time-based labels
    return telemetryPoints.map((point, index) => {
      let name = t ? t('chart.item', { count: index + 1 }) : `Item ${index + 1}`
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

export interface PieData {
  name: string
  value: number
  color?: string
}

export interface PieChartProps {
  // Data source configuration
  dataSource?: DataSourceOrList  // Support both single and multiple data sources

  // Data
  data?: PieData[]

  // Display options
  title?: string
  height?: number | 'auto'
  showLegend?: boolean
  showTooltip?: boolean
  showLabels?: boolean

  // Style
  variant?: 'pie' | 'donut'
  innerRadius?: number | string
  outerRadius?: number | string

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
  colors?: string[]
  size?: 'sm' | 'md' | 'lg'
  className?: string
}

export function PieChart({
  dataSource,
  data: propData,
  title,
  height = 'auto',
  showLegend = false,
  showTooltip = true,
  showLabels = false,
  variant = 'donut',
  innerRadius = '60%',
  outerRadius = '80%',
  limit = 10,
  timeRange = 1,
  aggregate = 'latest',  // Default to latest value for pie charts
  timeWindow,
  dataMapping,
  colors,
  size = 'md',
  className,
}: PieChartProps) {
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

  // Normalize data sources for telemetry
  const telemetrySources = useMemo(() => {
    const sources = normalizeDataSource(dataSource)
    return sources.map(ds => toTelemetrySource(ds, limit, timeRange)).filter((ds): ds is DataSource => ds !== undefined)
  }, [dataSource, limit, timeRange])

  const { data, loading } = useDataSource<PieData[] | number[] | number[][]>(
    telemetrySources.length > 0 ? (telemetrySources.length === 1 ? telemetrySources[0] : telemetrySources) : undefined,
    {
      fallback: propData ?? [
        { name: t('chart.categoryA'), value: 30 },
        { name: t('chart.categoryB'), value: 45 },
        { name: t('chart.categoryC'), value: 25 },
      ],
      preserveMultiple: true,
    }
  )

  // Get device names for labels
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

  // Normalize data to PieData[] format
  const chartData: PieData[] = useMemo(() => {
    const sources = normalizeDataSource(dataSource)
    const chartColors = colors || fallbackColors

    // Multi-source data - combine into single pie chart
    // preserveMultiple returns array of arrays where length equals sources length
    if (sources.length > 1 && Array.isArray(data) && data.length === sources.length) {
      return sources.map((ds, i) => {
        const arr = data[i]
        if (!Array.isArray(arr)) {
          return {
            name: ds.deviceId
              ? `${getDeviceName(ds.deviceId)} · ${getPropertyDisplayName(ds.metricId || ds.property)}`
              : t('chart.series', { count: i + 1 }),
            value: 0,
            color: chartColors[i % chartColors.length],
          }
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
            // Create timePoints for aggregation (use actual timestamps if available)
            const now = Date.now() / 1000
            const timePoints = (arr as Array<{ value?: unknown; timestamp?: number }>).map((item, idx) => ({
              timestamp: item.timestamp || (now - (values.length - idx) * 60),
              value: values[idx],
            }))

            const aggregatedValue = aggregateData(timePoints, effectiveAggregate)
            return {
              name: ds.deviceId
                ? `${getDeviceName(ds.deviceId)} · ${getPropertyDisplayName(ds.metricId || ds.property)}`
                : t('chart.series', { count: i + 1 }),
              value: aggregatedValue ?? 0,
              color: chartColors[i % chartColors.length],
            }
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
          return {
            name: ds.deviceId
              ? `${getDeviceName(ds.deviceId)} · ${getPropertyDisplayName(ds.metricId || ds.property)}`
              : t('chart.series', { count: i + 1 }),
            value: aggregatedValue ?? 0,
            color: chartColors[i % chartColors.length],
          }
        }

        return {
          name: ds.deviceId
            ? `${getDeviceName(ds.deviceId)} · ${getPropertyDisplayName(ds.metricId || ds.property)}`
            : t('chart.series', { count: i + 1 }),
          value: 0,
          color: chartColors[i % chartColors.length],
        }
      })
    }

    // Handle telemetry data FIRST (when dataSource is provided)
    if (dataSource && Array.isArray(data) && data.length > 0) {
      const first = data[0]
      // Check if already in PieData format (has both 'name' AND 'value')
      if (typeof first === 'object' && first !== null && 'value' in first && 'name' in first) {
        return data as PieData[]
      }

      // Transform telemetry points (handles both numeric and categorical data)
      // Pass aggregate setting to influence transformation behavior
      const transformed = transformTelemetryToPieData(data, dataMapping, effectiveAggregate, t)
      if (transformed.length > 0) {
        return transformed
      }
    }

    // Handle number array from data source - apply aggregation
    if (dataSource && Array.isArray(data) && data.length > 0 && typeof data[0] === 'number') {
      const values = data as number[]
      const timePoints = values.map((v, idx) => ({
        timestamp: Date.now() / 1000 - (values.length - idx) * 60,
        value: v,
      }))

      const aggregatedValue = aggregateData(timePoints, effectiveAggregate)
      return [{
        name: sources[0]?.deviceId ? getDeviceName(sources[0].deviceId) : 'Value',
        value: aggregatedValue ?? values[values.length - 1] ?? 0,
      }]
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
      { name: t('chart.categoryA'), value: 30 },
      { name: t('chart.categoryB'), value: 45 },
      { name: t('chart.categoryC'), value: 25 },
    ]
  }, [data, propData, dataSource, dataMapping, effectiveAggregate, loading, t, colors])

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

  if (chartData.length === 0) {
    return <EmptyState size={size} className={className} message={title ? `${title} - No Data Available` : undefined} />
  }

  const effectiveColors = colors || fallbackColors

  return (
    <div className={cn(dashboardCardBase, config.padding, className)}>
      {title && (
        <div className={cn('mb-3', indicatorFontWeight.title, config.titleText)}>{title}</div>
      )}
      <div className={cn('w-full', size === 'sm' ? 'h-[120px]' : size === 'md' ? 'h-[180px]' : 'h-[240px]')}>
        <ResponsiveContainer width="100%" height="100%">
          <RechartsPieChart margin={{ top: 0, right: 0, bottom: 0, left: 0 }}>
            <Pie
              data={chartData}
              cx="50%"
              cy="50%"
              labelLine={false}
              label={showLabels ? (entry) => `${entry.name}` : false}
              innerRadius={variant === 'donut' ? innerRadius : 0}
              outerRadius={outerRadius}
              dataKey="value"
            >
              {chartData.map((entry, index) => (
                <Cell
                  key={`cell-${index}`}
                  fill={entry.color || effectiveColors[index % effectiveColors.length]}
                  stroke="none"
                />
              ))}
            </Pie>
            {showTooltip && <Tooltip content={<ChartTooltip />} />}
            {showLegend && <Legend />}
          </RechartsPieChart>
        </ResponsiveContainer>
      </div>
    </div>
  )
}
