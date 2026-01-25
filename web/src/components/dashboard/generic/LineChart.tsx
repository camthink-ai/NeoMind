/**
 * Line Chart Component
 *
 * Unified with dashboard design system.
 */

import { useMemo } from 'react'
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

function toTelemetrySource(dataSource?: DataSource): DataSource | undefined {
  if (!dataSource) return undefined
  if (dataSource.type !== 'device' && dataSource.type !== 'metric') {
    return dataSource
  }
  return {
    type: 'telemetry',
    deviceId: dataSource.deviceId,
    metricId: dataSource.metricId,
    timeRange: 1,
    aggregate: 'avg',
    limit: 50,
  }
}

export interface SeriesData {
  name: string
  data: number[]
  color?: string
}

export interface LineChartProps {
  dataSource?: DataSource
  series?: SeriesData[]
  labels?: string[]
  title?: string
  height?: number | 'auto'
  showGrid?: boolean
  showLegend?: boolean
  showTooltip?: boolean
  smooth?: boolean
  fillArea?: boolean
  color?: string
  size?: 'sm' | 'md' | 'lg'
  className?: string
}

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

export function LineChart({
  dataSource,
  series: propSeries,
  labels,
  title,
  height = 'auto',
  showGrid = false,
  showLegend = false,
  showTooltip = true,
  smooth = true,
  fillArea = false,
  color,
  size = 'md',
  className,
}: LineChartProps) {
  const config = dashboardComponentSize[size]
  const telemetrySource = toTelemetrySource(dataSource)
  const { data, loading } = useDataSource<SeriesData[] | number | number[]>(telemetrySource, {
    fallback: propSeries ?? [],
  })

  const normalizedSeries = useMemo(() => {
    if (propSeries && Array.isArray(propSeries) && propSeries.length > 0 && propSeries[0]?.data) {
      return propSeries
    }
    if (Array.isArray(data) && data.length > 0) {
      const first = data[0]
      if (typeof first === 'object' && first !== null && 'data' in first && Array.isArray(first.data)) {
        return data as SeriesData[]
      }
    }
    if (typeof data === 'number') {
      return [{ name: 'Value', data: [data] }]
    }
    if (Array.isArray(data) && data.length > 0 && typeof data[0] === 'number') {
      return [{ name: 'Value', data: data as number[] }]
    }
    if (Array.isArray(data) && data.length > 0 && typeof data[0] === 'object') {
      const values = (data as any[]).map(item => item.value ?? item.y ?? item)
      if (values.some(v => typeof v === 'number')) {
        return [{ name: 'Value', data: values.filter(v => typeof v === 'number') as number[] }]
      }
    }
    return [{
      name: 'Sample',
      data: [10, 15, 12, 18, 14, 20, 16, 22, 19, 25]
    }]
  }, [data, propSeries])

  const series = normalizedSeries

  const chartData = labels
    ? labels.map((label, idx) => {
        const point: any = { name: label }
        series.forEach((s, i) => {
          point[`series${i}`] = s.data[idx] ?? null
        })
        return point
      })
    : series[0]?.data.map((_, i) => {
        const point: any = { name: i }
        series.forEach((s, idx) => {
          point[`series${idx}`] = s.data[i] ?? null
        })
        return point
      })

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

  // Empty state
  if (series.length === 0 || chartData.length === 0) {
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
              tick={{ fill: 'hsl(var(--muted-foreground))', fontSize: 12 }}
              interval="preserveStartEnd"
            />
            <YAxis
              axisLine={false}
              tickLine={false}
              tickMargin={10}
              width={32}
              tick={{ fill: 'hsl(var(--muted-foreground))', fontSize: 12 }}
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
 */

const DEFAULT_AREA_DATA: SeriesData[] = [{ name: 'Revenue', data: [12, 19, 15, 25, 22, 30, 28, 35, 32, 40, 38, 45] }]

export interface AreaChartProps {
  dataSource?: DataSource
  series?: SeriesData[]
  labels?: string[]
  title?: string
  height?: number | 'auto'
  showGrid?: boolean
  showLegend?: boolean
  showTooltip?: boolean
  smooth?: boolean
  color?: string
  size?: 'sm' | 'md' | 'lg'
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
  className,
}: AreaChartProps) {
  const config = dashboardComponentSize[size]
  const effectiveSeries = propSeries || DEFAULT_AREA_DATA
  const telemetrySource = toTelemetrySource(dataSource)
  const shouldFetch = !!dataSource
  const { data: sourceData, loading } = useDataSource<SeriesData[] | number | number[]>(
    shouldFetch ? telemetrySource : undefined,
    shouldFetch ? { fallback: effectiveSeries } : undefined
  )
  const rawData = shouldFetch ? sourceData : effectiveSeries

  const normalizedSeries = useMemo(() => {
    if (propSeries && Array.isArray(propSeries) && propSeries.length > 0 && propSeries[0]?.data) {
      return propSeries
    }
    if (Array.isArray(rawData) && rawData.length > 0) {
      const first = rawData[0]
      if (typeof first === 'object' && first !== null && 'data' in first && Array.isArray(first.data)) {
        return rawData as SeriesData[]
      }
    }
    if (typeof rawData === 'number') {
      return [{ name: 'Value', data: [rawData] }]
    }
    if (Array.isArray(rawData) && rawData.length > 0 && typeof rawData[0] === 'number') {
      return [{ name: 'Value', data: rawData as number[] }]
    }
    if (Array.isArray(rawData) && rawData.length > 0 && typeof rawData[0] === 'object') {
      const values = (rawData as any[]).map(item => item.value ?? item.y ?? item)
      if (values.some(v => typeof v === 'number')) {
        return [{ name: 'Value', data: values.filter(v => typeof v === 'number') as number[] }]
      }
    }
    return DEFAULT_AREA_DATA
  }, [rawData, propSeries])

  const series = normalizedSeries

  const chartData = labels
    ? labels.map((label, idx) => {
        const point: any = { name: label }
        series.forEach((s, i) => {
          point[`series${i}`] = s.data[idx] ?? null
        })
        return point
      })
    : series[0]?.data.map((_, i) => {
        const point: any = { name: i }
        series.forEach((s, idx) => {
          point[`series${idx}`] = s.data[i] ?? null
        })
        return point
      })

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

  // Empty state
  if (series.length === 0 || chartData.length === 0) {
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
          <RechartsAreaChart data={chartData} margin={{ top: 5, right: 5, bottom: 0, left: 0 }} accessibilityLayer>
            {showGrid && <CartesianGrid vertical={false} strokeDasharray="4 4" className="stroke-muted" />}
            <XAxis
              dataKey="name"
              axisLine={false}
              tickLine={false}
              tickMargin={10}
              tick={{ fill: 'hsl(var(--muted-foreground))', fontSize: 12 }}
              interval="preserveStartEnd"
            />
            <YAxis
              axisLine={false}
              tickLine={false}
              tickMargin={10}
              width={32}
              tick={{ fill: 'hsl(var(--muted-foreground))', fontSize: 12 }}
            />
            {showTooltip && <Tooltip content={<ChartTooltip />} />}
            {showLegend && <Legend />}
            {series.map((s, i) => {
              const isCssVariable = s.color && (s.color.startsWith('hsl') || s.color.startsWith('var('))
              const seriesColor = isCssVariable ? fallbackColors[i % fallbackColors.length] : (s.color || fallbackColors[i % fallbackColors.length])
              return (
                <Area
                  key={i}
                  type={smooth ? 'monotone' : 'linear'}
                  dataKey={`series${i}`}
                  name={s.name}
                  stroke={seriesColor}
                  strokeWidth={2}
                  fill={seriesColor}
                  fillOpacity={0.7}
                />
              )
            })}
          </RechartsAreaChart>
        </ResponsiveContainer>
      </div>
    </div>
  )
}
