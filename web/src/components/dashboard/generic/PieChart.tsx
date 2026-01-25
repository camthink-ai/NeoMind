/**
 * Pie Chart Component
 *
 * Unified with dashboard design system.
 */

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
  '#06b6d4', // Cyan
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
  colors,
  size = 'md',
  className,
}: PieChartProps) {
  const config = dashboardComponentSize[size]
  // Get data from source
  const { data, loading } = useDataSource<PieData[]>(dataSource, {
    fallback: propData ?? [
      { name: 'Category A', value: 30 },
      { name: 'Category B', value: 45 },
      { name: 'Category C', value: 25 },
    ],
  })

  const chartData = data ?? propData ?? [
    { name: 'Category A', value: 30 },
    { name: 'Category B', value: 45 },
    { name: 'Category C', value: 25 },
  ]

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
