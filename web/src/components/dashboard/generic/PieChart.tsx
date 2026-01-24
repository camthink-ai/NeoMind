/**
 * Pie Chart Component
 *
 * shadcn/ui inspired pie/donut chart.
 * Clean design with donut variant and subtle colors.
 */

import {
  PieChart as RechartsPieChart,
  Pie,
  Cell,
  ResponsiveContainer,
  Tooltip,
  Legend,
} from 'recharts'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import type { DataSource } from '@/types/dashboard'

export interface PieData {
  name: string
  value: number
  color?: string
}

export interface PieChartProps {
  // Data source configuration
  dataSource?: DataSource

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

  // Styling
  colors?: string[]
  className?: string
}

// shadcn/ui chart colors - using CSS variables
const defaultColors = [
  'var(--color-chart-1)',
  'var(--color-chart-2)',
  'var(--color-chart-3)',
  'var(--color-chart-4)',
  'var(--color-chart-5)',
  'var(--color-chart-6)',
]

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
  colors,
  className,
}: PieChartProps) {
  // Get data from source
  const { data, loading } = useDataSource<PieData[]>(dataSource, {
    fallback: propData ?? [],
  })

  const chartData = data ?? propData ?? []

  if (loading) {
    return (
      <Card className={cn('border shadow-sm', className)}>
        {title && <CardHeader><CardTitle className="text-sm font-medium">{title}</CardTitle></CardHeader>}
        <CardContent>
          <Skeleton className="w-full h-full" />
        </CardContent>
      </Card>
    )
  }

  if (chartData.length === 0) {
    return (
      <Card className={cn('border shadow-sm', className)}>
        {title && <CardHeader><CardTitle className="text-sm font-medium">{title}</CardTitle></CardHeader>}
        <CardContent>
          <div className="flex items-center justify-center h-full text-sm text-muted-foreground">
            No data available
          </div>
        </CardContent>
      </Card>
    )
  }

  const chartColors = colors || defaultColors

  return (
    <Card className={cn('border shadow-sm overflow-hidden flex flex-col h-full', className)}>
      {title && <CardHeader className="pb-2 flex-shrink-0"><CardTitle className="text-sm font-medium">{title}</CardTitle></CardHeader>}
      <CardContent className={cn(!title && 'pt-6', 'flex-1 min-h-0 p-4')}>
        <style>{`
          :root {
            --color-chart-1: oklch(0.646 0.222 264.38);
            --color-chart-2: oklch(0.646 0.222 142.5);
            --color-chart-3: oklch(0.646 0.222 48.85);
            --color-chart-4: oklch(0.646 0.222 24.85);
            --color-chart-5: oklch(0.646 0.222 304.38);
            --color-chart-6: oklch(0.646 0.222 188.38);
          }
        `}</style>
        <ResponsiveContainer width="100%" height={height === 'auto' ? '100%' : height}>
          <RechartsPieChart margin={{ top: 0, right: 0, bottom: 0, left: 0 }}>
            <Pie
              data={chartData}
              cx="50%"
              cy="50%"
              labelLine={false}
              label={showLabels ? (entry) => `${entry.name}` : false}
              innerRadius={variant === 'donut' ? innerRadius : 0}
              outerRadius={outerRadius}
              paddingAngle={chartData.length > 1 ? 0 : 0}
              dataKey="value"
              className="outline-none"
            >
              {chartData.map((entry, index) => (
                <Cell
                  key={`cell-${index}`}
                  fill={entry.color || chartColors[index % chartColors.length]}
                  stroke="hsl(var(--background))"
                  strokeWidth={2}
                  className="transition-opacity hover:opacity-80"
                />
              ))}
            </Pie>
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
          </RechartsPieChart>
        </ResponsiveContainer>
      </CardContent>
    </Card>
  )
}

// DonutChart is an alias for PieChart with variant='donut'
export function DonutChart(props: Omit<PieChartProps, 'variant'>) {
  return <PieChart {...props} variant="donut" />
}
