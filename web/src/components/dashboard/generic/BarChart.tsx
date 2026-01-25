/**
 * Bar Chart Component
 *
 * Unified with dashboard design system.
 * Supports historical telemetry data binding.
 */

import { useMemo } from 'react'

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
import { useDataSource } from '@/hooks/useDataSource'
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
import { indicatorFontWeight } from '@/design-system/tokens/indicator'
import { chartColors as designChartColors } from '@/design-system/tokens/color'
import type { DataSource, DataSourceOrList } from '@/types/dashboard'
import { normalizeDataSource } from '@/types/dashboard'

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
 */
function toTelemetrySource(
  dataSource?: DataSource,
  limit: number = 24,
  timeRange: number = 1
): DataSource | undefined {
  if (!dataSource) {
    return undefined
  }

  // If already telemetry type, update with raw settings
  if (dataSource.type === 'telemetry') {
    return {
      ...dataSource,
      limit: dataSource.limit ?? limit,
      timeRange: dataSource.timeRange ?? timeRange,
      aggregate: 'raw',
      params: {
        ...dataSource.params,
        includeRawPoints: true,
      },
      transform: 'raw',
    }
  }

  // Convert to telemetry for historical data
  if (dataSource.type === 'device' || dataSource.type === 'metric') {
    return {
      type: 'telemetry' as const,
      deviceId: dataSource.deviceId,
      metricId: dataSource.metricId ?? dataSource.property ?? 'value',
      timeRange: timeRange,
      limit: limit,
      aggregate: 'raw' as const,
      params: {
        includeRawPoints: true,
      },
      transform: 'raw' as const,
    }
  }

  return dataSource
}

/**
 * Transform raw telemetry points to chart data
 */
function transformTelemetryToBarData(data: unknown) {
  if (!data || !Array.isArray(data)) return []

  const result: { name: string; value: number; color?: string }[] = []

  for (const item of data) {
    if (typeof item === 'number') {
      result.push({ name: `${result.length + 1}`, value: item })
    } else if (typeof item === 'object' && item !== null) {
      const obj = item as Record<string, unknown>

      // Extract value
      let value: number | undefined = undefined
      if (typeof obj.value === 'number') value = obj.value
      else if (typeof obj.v === 'number') value = obj.v
      else if (typeof obj.val === 'number') value = obj.val
      else if (typeof obj.avg === 'number') value = obj.avg
      else if (typeof obj.min === 'number') value = obj.min
      else if (typeof obj.max === 'number') value = obj.max

      if (value === undefined) continue

      // Extract timestamp/name for label
      const timestamp = obj.timestamp ?? obj.time ?? obj.t
      const name = obj.name ?? obj.label

      let label = `${result.length + 1}`
      if (typeof name === 'string') {
        label = name
      } else if (typeof timestamp === 'number') {
        const date = new Date(timestamp * 1000)
        label = date.toLocaleTimeString('zh-CN', {
          hour: '2-digit',
          minute: '2-digit',
        })
      } else if (typeof timestamp === 'string') {
        label = timestamp
      }

      result.push({ name: label, value })
    }
  }

  return result
}

export interface BarData {
  name: string
  value: number
  color?: string
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

  // Telemetry options
  limit?: number
  timeRange?: number

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
  className,
}: BarChartProps) {
  const config = dashboardComponentSize[size]

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
    if (!deviceId) return 'Value'
    return deviceId.replace(/[-_]/g, ' ').replace(/\b\w/g, c => c.toUpperCase())
  }

  const getPropertyDisplayName = (property?: string): string => {
    if (!property) return 'Value'
    const propertyNames: Record<string, string> = {
      temperature: '温度',
      humidity: '湿度',
      temp: '温度',
      value: '数值',
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

    // Multi-source data - create grouped bar chart
    // preserveMultiple returns array of arrays where length equals sources length
    if (sources.length > 1 && Array.isArray(data) && data.length === sources.length) {
      const numberArrays = data as number[][]
      const maxLength = Math.max(...numberArrays.map(arr => Array.isArray(arr) ? arr.length : 0))

      return Array.from({ length: maxLength }, (_, idx) => {
        const point: any = { name: `${idx + 1}` }
        sources.forEach((ds, i) => {
          const sourceArray = numberArrays[i]
          const arrValue = Array.isArray(sourceArray) ? sourceArray[idx] : 0
          const seriesKey = `series${i}`
          point[seriesKey] = arrValue ?? 0
          // Also store the display name for this series key
          if (i === 0) {
            point.seriesNames = sources.map((ds, si) => {
              return ds.deviceId
                ? `${getDeviceName(ds.deviceId)} · ${getPropertyDisplayName(ds.property)}`
                : `Series ${si + 1}`
            })
          }
        })
        return point
      })
    }

    // Single source - handle as before
    if (dataSource && Array.isArray(data) && data.length > 0) {
      const first = data[0]
      if (typeof first === 'object' && first !== null && 'value' in first) {
        return data as BarData[]
      }

      // Transform telemetry points
      const transformed = transformTelemetryToBarData(data)
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

    // Return empty array when loading with dataSource
    if (dataSource && loading) {
      return []
    }

    // Return default sample data for preview only
    return [
      { name: 'Jan', value: 12 },
      { name: 'Feb', value: 18 },
      { name: 'Mar', value: 15 },
      { name: 'Apr', value: 22 },
      { name: 'May', value: 19 },
      { name: 'Jun', value: 25 },
    ]
  }, [data, propData, dataSource, loading])

  // Get series info for multi-source rendering
  const seriesInfo = useMemo(() => {
    const sources = normalizeDataSource(dataSource)
    if (sources.length > 1 && Array.isArray(data) && data.length === sources.length) {
      return sources.map((ds, i) => ({
        dataKey: `series${i}`,
        name: ds.deviceId
          ? `${getDeviceName(ds.deviceId)} · ${getPropertyDisplayName(ds.property)}`
          : `Series ${i + 1}`,
        color: fallbackColors[i % fallbackColors.length],
      }))
    }
    return null
  }, [dataSource, data])

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
    return (
      <div className={cn(dashboardCardBase, 'flex items-center justify-center', config.padding, className)}>
        <span className={cn('text-sm text-muted-foreground')}>No data available</span>
      </div>
    )
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
                {chartData.map((entry, index) => (
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
