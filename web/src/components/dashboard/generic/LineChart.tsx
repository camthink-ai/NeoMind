/**
 * Line Chart Component
 *
 * shadcn/ui inspired line chart with clean design.
 * Minimal grid, smooth curves, gradient fills.
 */

import {
  LineChart as RechartsLineChart,
  Line,
  Area,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
  ResponsiveContainer,
} from 'recharts'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import type { DataSource } from '@/types/dashboard'

export interface SeriesData {
  name: string
  data: number[]
  color?: string
}

export interface LineChartProps {
  // Data source configuration
  dataSource?: DataSource

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

  // Styling
  color?: string
  className?: string
}

// shadcn/ui chart colors - using CSS variables
const defaultColors = [
  'var(--color-chart-1)',
  'var(--color-chart-2)',
  'var(--color-chart-3)',
  'var(--color-chart-4)',
  'var(--color-chart-5)',
]

export function LineChart({
  dataSource,
  series: propSeries,
  labels,
  title,
  height = 'auto',
  showGrid = true,
  showLegend = false,
  showTooltip = true,
  smooth = true,
  fillArea = false,
  color,
  className,
}: LineChartProps) {
  const { data, loading } = useDataSource<SeriesData[]>(dataSource, {
    fallback: propSeries ?? [],
  })

  const series = data ?? propSeries ?? []

  // Convert series data to recharts format
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

  if (loading) {
    return (
      <Card className={cn('border shadow-sm', className)}>
        {title && <CardHeader><CardTitle className="text-sm font-medium">{title}</CardTitle></CardHeader>}
        <CardContent>
          <Skeleton className="w-full h-[200px]" />
        </CardContent>
      </Card>
    )
  }

  if (series.length === 0 || chartData.length === 0) {
    return (
      <Card className={cn('border shadow-sm', className)}>
        {title && <CardHeader><CardTitle className="text-sm font-medium">{title}</CardTitle></CardHeader>}
        <CardContent>
          <div className="flex items-center justify-center h-[200px] text-sm text-muted-foreground">
            No data available
          </div>
        </CardContent>
      </Card>
    )
  }

  return (
    <Card className={cn(
      'dashboardCardBase',
      className
    )}>
      {title && <CardHeader className="pb-2 flex-shrink-0"><CardTitle className="text-sm font-medium">{title}</CardTitle></CardHeader>}
      <CardContent className={cn(!title && 'pt-6', 'flex-1 min-h-0 p-4')}>
        <style>{`
          :root {
            --color-chart-1: oklch(0.646 0.222 264.38);
            --color-chart-2: oklch(0.646 0.222 142.5);
            --color-chart-3: oklch(0.646 0.222 48.85);
            --color-chart-4: oklch(0.646 0.222 24.85);
            --color-chart-5: oklch(0.646 0.222 304.38);
          }
        `}</style>
        <ResponsiveContainer width="100%" height={height === 'auto' ? '100%' : height}>
          <RechartsLineChart data={chartData} margin={{ top: 10, right: 10, bottom: 0, left: 0 }}>
            <defs>
              {series.map((s, i) => {
                const seriesColor = s.color || color || defaultColors[i % defaultColors.length]
                return (
                  <linearGradient key={i} id={`gradient-${i}`} x1="0" y1="0" x2="0" y2="1">
                    <stop offset="5%" stopColor={seriesColor} stopOpacity={0.3} />
                    <stop offset="95%" stopColor={seriesColor} stopOpacity={0} />
                  </linearGradient>
                )
              })}
            </defs>
            {showGrid && <CartesianGrid vertical={false} strokeDasharray="4 4" className="stroke-muted" />}
            <XAxis
              dataKey="name"
              className="text-xs text-muted-foreground"
              axisLine={false}
              tickLine={false}
              tickMargin={8}
            />
            <YAxis
              className="text-xs text-muted-foreground"
              axisLine={false}
              tickLine={false}
              tickMargin={8}
              width={40}
            />
            {showTooltip && (
              <Tooltip
                content={({ active, payload }) => {
                  if (!active || !payload?.length) return null
                  return (
                    <div className="rounded-lg border bg-background p-2 shadow-sm">
                      {payload.map((entry: any, index: number) => (
                        <div key={index} className="flex items-center gap-2 text-xs">
                          <div
                            className="h-2 w-2 rounded-full"
                            style={{ backgroundColor: entry.color }}
                          />
                          <span className="text-muted-foreground">{entry.name}:</span>
                          <span className="font-medium tabular-nums">{entry.value}</span>
                        </div>
                      ))}
                    </div>
                  )
                }}
              />
            )}
            {showLegend && <Legend />}
            {series.map((s, i) => {
              const seriesColor = s.color || color || defaultColors[i % defaultColors.length]
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
      </CardContent>
    </Card>
  )
}

/**
 * Area Chart Component
 *
 * Similar to LineChart but with filled area under the line.
 */
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
  className?: string
}

export function AreaChart({
  dataSource,
  series: propSeries,
  labels,
  title,
  height = 'auto',
  showGrid = true,
  showLegend = false,
  showTooltip = true,
  smooth = true,
  color,
  className,
}: AreaChartProps) {
  const { data, loading } = useDataSource<SeriesData[]>(dataSource, {
    fallback: propSeries ?? [],
  })

  const series = data ?? propSeries ?? []

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

  if (loading) {
    return (
      <Card className={cn('border shadow-sm', className)}>
        {title && <CardHeader><CardTitle className="text-sm font-medium">{title}</CardTitle></CardHeader>}
        <CardContent>
          <Skeleton className="w-full h-[200px]" />
        </CardContent>
      </Card>
    )
  }

  if (series.length === 0 || chartData.length === 0) {
    return (
      <Card className={cn('border shadow-sm', className)}>
        {title && <CardHeader><CardTitle className="text-sm font-medium">{title}</CardTitle></CardHeader>}
        <CardContent>
          <div className="flex items-center justify-center h-[200px] text-sm text-muted-foreground">
            No data available
          </div>
        </CardContent>
      </Card>
    )
  }

  return (
    <Card className={cn(
      'dashboardCardBase',
      className
    )}>
      {title && <CardHeader className="pb-2 flex-shrink-0"><CardTitle className="text-sm font-medium">{title}</CardTitle></CardHeader>}
      <CardContent className={cn(!title && 'pt-6', 'flex-1 min-h-0 p-4')}>
        <style>{`
          :root {
            --color-chart-1: oklch(0.646 0.222 264.38);
            --color-chart-2: oklch(0.646 0.222 142.5);
            --color-chart-3: oklch(0.646 0.222 48.85);
            --color-chart-4: oklch(0.646 0.222 24.85);
            --color-chart-5: oklch(0.646 0.222 304.38);
          }
        `}</style>
        <ResponsiveContainer width="100%" height={height === 'auto' ? '100%' : height}>
          <RechartsLineChart data={chartData} margin={{ top: 10, right: 10, bottom: 0, left: 0 }}>
            <defs>
              {series.map((s, i) => {
                const seriesColor = s.color || color || defaultColors[i % defaultColors.length]
                return (
                  <linearGradient key={i} id={`area-gradient-${i}`} x1="0" y1="0" x2="0" y2="1">
                    <stop offset="5%" stopColor={seriesColor} stopOpacity={0.5} />
                    <stop offset="95%" stopColor={seriesColor} stopOpacity={0.05} />
                  </linearGradient>
                )
              })}
            </defs>
            {showGrid && <CartesianGrid vertical={false} strokeDasharray="4 4" className="stroke-muted" />}
            <XAxis
              dataKey="name"
              className="text-xs text-muted-foreground"
              axisLine={false}
              tickLine={false}
              tickMargin={8}
            />
            <YAxis
              className="text-xs text-muted-foreground"
              axisLine={false}
              tickLine={false}
              tickMargin={8}
              width={40}
            />
            {showTooltip && (
              <Tooltip
                content={({ active, payload }) => {
                  if (!active || !payload?.length) return null
                  return (
                    <div className="rounded-lg border bg-background p-2 shadow-sm">
                      {payload.map((entry: any, index: number) => (
                        <div key={index} className="flex items-center gap-2 text-xs">
                          <div
                            className="h-2 w-2 rounded-full"
                            style={{ backgroundColor: entry.color }}
                          />
                          <span className="text-muted-foreground">{entry.name}:</span>
                          <span className="font-medium tabular-nums">{entry.value}</span>
                        </div>
                      ))}
                    </div>
                  )
                }}
              />
            )}
            {showLegend && <Legend />}
            {series.map((s, i) => {
              const seriesColor = s.color || color || defaultColors[i % defaultColors.length]
              return (
                <g key={i}>
                  <Area
                    type={smooth ? 'monotone' : 'linear'}
                    dataKey={`series${i}`}
                    name={s.name}
                    stroke={seriesColor}
                    fill={`url(#area-gradient-${i})`}
                    strokeWidth={2}
                  />
                  <Line
                    type={smooth ? 'monotone' : 'linear'}
                    dataKey={`series${i}`}
                    name={s.name}
                    stroke={seriesColor}
                    strokeWidth={2}
                    dot={false}
                    activeDot={{ r: 4 }}
                    fill="none"
                    strokeLinejoin="round"
                    strokeLinecap="round"
                  />
                </g>
              )
            })}
          </RechartsLineChart>
        </ResponsiveContainer>
      </CardContent>
    </Card>
  )
}
