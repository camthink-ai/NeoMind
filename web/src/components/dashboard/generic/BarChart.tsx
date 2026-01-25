/**
 * Bar Chart Component
 *
 * Unified with dashboard design system.
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
import type { DataSource } from '@/types/dashboard'

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
function toTelemetrySource(dataSource?: DataSource): DataSource | undefined {
  if (!dataSource) return undefined

  // Already telemetry or other types - use as-is
  if (dataSource.type !== 'device' && dataSource.type !== 'metric') {
    return dataSource
  }

  // Convert to telemetry for historical data
  return {
    type: 'telemetry',
    deviceId: dataSource.deviceId,
    metricId: dataSource.metricId,
    timeRange: 1,         // Last 1 hour
    aggregate: 'avg',     // Aggregate data points
    limit: 24,            // 24 bars for readable display
  }
}

export interface BarData {
  name: string
  value: number
  color?: string
}

export interface BarChartProps {
  // Data source configuration
  dataSource?: DataSource

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
  className,
}: BarChartProps) {
  const config = dashboardComponentSize[size]
  // Use telemetry source for time-series data
  const telemetrySource = toTelemetrySource(dataSource)
  const { data, loading } = useDataSource<BarData[] | number[]>(telemetrySource, {
    fallback: propData ?? [],
  })

  // Normalize data to BarData[] format
  const chartData: BarData[] = useMemo(() => {
    // If propData is provided and valid, use it
    if (propData && Array.isArray(propData) && propData.length > 0) {
      return propData
    }

    // If data from source is already in BarData[] format
    if (Array.isArray(data) && data.length > 0 && typeof data[0] === 'object' && data[0] !== null && 'value' in data[0]) {
      return data as BarData[]
    }

    // If data is a number array, convert to BarData[]
    if (Array.isArray(data) && data.length > 0 && typeof data[0] === 'number') {
      return (data as number[]).map((value, index) => ({
        name: `${index + 1}`,
        value,
      }))
    }

    // Return default sample data instead of empty array
    return [
      { name: 'Jan', value: 12 },
      { name: 'Feb', value: 18 },
      { name: 'Mar', value: 15 },
      { name: 'Apr', value: 22 },
      { name: 'May', value: 19 },
      { name: 'Jun', value: 25 },
    ]
  }, [data, propData])

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

  return (
    <div className={cn(dashboardCardBase, config.padding, className)}>
      {title && (
        <div className={cn('mb-3', indicatorFontWeight.title, config.titleText)}>{title}</div>
      )}
      <div className={cn('w-full', size === 'sm' ? 'h-[120px]' : size === 'md' ? 'h-[180px]' : 'h-[240px]')}>
        <ResponsiveContainer width="100%" height="100%">
          <RechartsBarChart
            data={chartData}
            layout={layout === 'horizontal' ? 'vertical' : 'horizontal'}
            margin={{ top: 5, right: 5, bottom: 5, left: 0 }}
            accessibilityLayer
          >
            {showGrid && <CartesianGrid strokeDasharray="3 3" className="stroke-muted" vertical={false} />}
            <XAxis
              dataKey={layout === 'vertical' ? 'name' : undefined}
              type={layout === 'vertical' ? 'category' : 'number'}
              axisLine={false}
              tickLine={false}
              tickMargin={10}
              tick={{ fill: 'hsl(var(--muted-foreground))', fontSize: 12 }}
            />
            <YAxis
              dataKey={layout === 'horizontal' ? 'name' : undefined}
              type={layout === 'horizontal' ? 'category' : 'number'}
              axisLine={false}
              tickLine={false}
              tickMargin={10}
              width={layout === 'horizontal' ? 60 : 40}
              tick={{ fill: 'hsl(var(--muted-foreground))', fontSize: 12 }}
            />
            {showTooltip && <Tooltip content={<ChartTooltip />} />}
            {showLegend && <Legend />}
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
          </RechartsBarChart>
        </ResponsiveContainer>
      </div>
    </div>
  )
}
