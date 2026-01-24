/**
 * Bar Chart Component
 *
 * A shadcn/ui compliant bar chart using Recharts.
 * Supports data binding and real-time updates.
 * Fully responsive and adaptive.
 */

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
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import type { DataSource } from '@/types/dashboard'

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
  className?: string
}

const defaultColors = [
  'hsl(var(--chart-1))',
  'hsl(var(--chart-2))',
  'hsl(var(--chart-3))',
  'hsl(var(--chart-4))',
  'hsl(var(--chart-5))',
]

export function BarChart({
  dataSource,
  data: propData,
  title,
  height = 'auto',
  showGrid = true,
  showLegend = false,
  showTooltip = true,
  layout = 'vertical',
  color,
  className,
}: BarChartProps) {
  // Get data from source
  const { data, loading } = useDataSource<BarData[]>(dataSource, {
    fallback: propData ?? [],
  })

  const chartData = data ?? propData ?? []

  if (loading) {
    return (
      <Card className={cn('overflow-hidden flex flex-col h-full', className)}>
        {title && <CardHeader className="pb-2 flex-shrink-0"><CardTitle className="text-sm">{title}</CardTitle></CardHeader>}
        <CardContent className="flex-1 min-h-0 p-4">
          <Skeleton className="w-full h-full" />
        </CardContent>
      </Card>
    )
  }

  if (chartData.length === 0) {
    return (
      <Card className={cn('overflow-hidden flex flex-col h-full', className)}>
        {title && <CardHeader className="pb-2 flex-shrink-0"><CardTitle className="text-sm">{title}</CardTitle></CardHeader>}
        <CardContent className="flex-1 min-h-0 p-4">
          <div className="flex items-center justify-center h-full text-sm text-muted-foreground">
            No data available
          </div>
        </CardContent>
      </Card>
    )
  }

  return (
    <Card className={cn('overflow-hidden flex flex-col h-full', className)}>
      {title && <CardHeader className="pb-2 flex-shrink-0"><CardTitle className="text-sm">{title}</CardTitle></CardHeader>}
      <CardContent className={cn(!title && 'pt-6', 'flex-1 min-h-0 p-4')}>
        <ResponsiveContainer width="100%" height={height === 'auto' ? '100%' : height}>
          <RechartsBarChart
            data={chartData}
            layout={layout === 'horizontal' ? 'vertical' : 'horizontal'}
            margin={{ top: 5, right: 5, bottom: 5, left: 0 }}
          >
            {showGrid && <CartesianGrid strokeDasharray="3 3" className="stroke-muted" />}
            <XAxis
              dataKey={layout === 'vertical' ? 'name' : undefined}
              type={layout === 'vertical' ? 'category' : 'number'}
              className="text-xs"
              axisLine={false}
              tickLine={false}
            />
            <YAxis
              dataKey={layout === 'horizontal' ? 'name' : undefined}
              type={layout === 'horizontal' ? 'category' : 'number'}
              className="text-xs"
              axisLine={false}
              tickLine={false}
              width={layout === 'horizontal' ? 60 : 40}
            />
            {showTooltip && <Tooltip />}
            {showLegend && <Legend />}
            <Bar
              dataKey="value"
              fill={color || defaultColors[0]}
              radius={[4, 4, 0, 0]}
            >
              {chartData.map((entry, index) => (
                <Cell
                  key={`cell-${index}`}
                  fill={entry.color || color || defaultColors[index % defaultColors.length]}
                />
              ))}
            </Bar>
          </RechartsBarChart>
        </ResponsiveContainer>
      </CardContent>
    </Card>
  )
}
