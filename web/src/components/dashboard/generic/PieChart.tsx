/**
 * Pie Chart Component
 *
 * Unified with dashboard design system.
 * Supports telemetry data binding for categorical/part-to-whole data.
 */

import { useMemo } from 'react'
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
  '#06b6d4', // Cyan
]

/**
 * Convert device/metric source to telemetry for pie chart data.
 * For pie charts, we typically want the latest snapshot or aggregated categories.
 */
function toTelemetrySource(
  dataSource?: DataSource,
  limit: number = 10,
  timeRange: number = 1
): DataSource | undefined {
  if (!dataSource) return undefined

  // If already telemetry type, update with settings
  if (dataSource.type === 'telemetry') {
    return {
      ...dataSource,
      limit: dataSource.limit ?? limit,
      timeRange: dataSource.timeRange ?? timeRange,
      aggregate: dataSource.aggregate ?? 'raw',
      params: {
        ...dataSource.params,
        includeRawPoints: true,
      },
      transform: dataSource.transform ?? 'raw',
    }
  }

  // Convert device/metric to telemetry
  if (dataSource.type === 'device' || dataSource.type === 'metric') {
    return {
      type: 'telemetry',
      deviceId: dataSource.deviceId,
      metricId: dataSource.metricId ?? dataSource.property ?? 'value',
      timeRange: timeRange,
      limit: limit,
      aggregate: 'raw',
      params: {
        includeRawPoints: true,
      },
      transform: 'raw',
    }
  }

  return dataSource
}

/**
 * Transform telemetry points to pie chart data.
 * Handles: [{ name, value }, { label, val }, { category, count }] or raw numbers
 */
function transformTelemetryToPieData(data: unknown): PieData[] {
  if (!data || !Array.isArray(data)) return []

  const result: PieData[] = []

  for (const item of data) {
    if (typeof item === 'number') {
      result.push({ name: `Item ${result.length + 1}`, value: item })
    } else if (typeof item === 'object' && item !== null) {
      const obj = item as Record<string, unknown>

      // Extract value
      let value: number | undefined = undefined
      if (typeof obj.value === 'number') value = obj.value
      else if (typeof obj.v === 'number') value = obj.v
      else if (typeof obj.val === 'number') value = obj.val
      else if (typeof obj.count === 'number') value = obj.count
      else if (typeof obj.amount === 'number') value = obj.amount

      if (value === undefined) continue

      // Extract name/label for category
      const name = obj.name ?? obj.label ?? obj.category ?? obj.key ?? `Item ${result.length + 1}`
      const label = typeof name === 'string' ? name : String(name)

      // Extract color if present
      const color = typeof obj.color === 'string' ? obj.color : undefined

      result.push({ name: label, value, color })
    }
  }

  return result
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

  // Telemetry options
  limit?: number
  timeRange?: number

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
  colors,
  size = 'md',
  className,
}: PieChartProps) {
  const config = dashboardComponentSize[size]

  // Normalize data sources for telemetry
  const telemetrySources = useMemo(() => {
    const sources = normalizeDataSource(dataSource)
    return sources.map(ds => toTelemetrySource(ds, limit, timeRange)).filter((ds): ds is DataSource => ds !== undefined)
  }, [dataSource, limit, timeRange])

  const { data, loading } = useDataSource<PieData[] | number[] | number[][]>(
    telemetrySources.length > 0 ? (telemetrySources.length === 1 ? telemetrySources[0] : telemetrySources) : undefined,
    {
      fallback: propData ?? [
        { name: 'Category A', value: 30 },
        { name: 'Category B', value: 45 },
        { name: 'Category C', value: 25 },
      ],
      preserveMultiple: true,
    }
  )

  // Get device names for labels
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

  // Normalize data to PieData[] format
  const chartData: PieData[] = useMemo(() => {
    const sources = normalizeDataSource(dataSource)

    // Multi-source data - combine into single pie chart
    // preserveMultiple returns array of arrays where length equals sources length
    if (sources.length > 1 && Array.isArray(data) && data.length === sources.length) {
      const numberArrays = data as number[][]
      // For pie chart, we sum each series and show as a slice
      return sources.map((ds, i) => {
        const arr = numberArrays[i]
        const values = Array.isArray(arr) ? arr : []
        const sum = values.reduce((a, b) => a + b, 0)
        return {
          name: ds.deviceId
            ? `${getDeviceName(ds.deviceId)} · ${getPropertyDisplayName(ds.property)}`
            : `Series ${i + 1}`,
          value: sum,
          color: fallbackColors[i % fallbackColors.length],
        }
      })
    }

    // Handle telemetry data FIRST (when dataSource is provided)
    if (dataSource && Array.isArray(data) && data.length > 0) {
      const first = data[0]
      if (typeof first === 'object' && first !== null && 'value' in first) {
        return data as PieData[]
      }

      // Transform telemetry points
      const transformed = transformTelemetryToPieData(data)
      if (transformed.length > 0) {
        return transformed
      }
    }

    // Handle number array from data source
    if (dataSource && Array.isArray(data) && data.length > 0 && typeof data[0] === 'number') {
      return (data as number[]).map((value, index) => ({
        name: `Item ${index + 1}`,
        value,
      }))
    }

    // If no dataSource, use propData (static data)
    if (!dataSource && propData && Array.isArray(propData) && propData.length > 0) {
      return propData
    }

    // Return default sample data
    return [
      { name: 'Category A', value: 30 },
      { name: 'Category B', value: 45 },
      { name: 'Category C', value: 25 },
    ]
  }, [data, propData, dataSource])

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
    return (
      <div className={cn(dashboardCardBase, 'flex items-center justify-center', config.padding, className)}>
        <span className={cn('text-sm text-muted-foreground')}>No data available</span>
      </div>
    )
  }

  const chartColors = colors || fallbackColors

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
                  fill={entry.color || chartColors[index % chartColors.length]}
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
