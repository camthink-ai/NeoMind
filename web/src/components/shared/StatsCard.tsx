import { cva, type VariantProps } from 'class-variance-authority'
import { cn } from '@/lib/utils'
import { TrendingUp, TrendingDown } from 'lucide-react'

const statsCardVariants = cva(
  'flex items-center gap-3 p-4 rounded-lg border bg-card/50 backdrop-blur-sm transition-all duration-200',
  {
    variants: {
      variant: {
        default: 'hover:shadow-md',
        success: 'bg-success/5 border-success/20 hover:bg-success/10 hover:shadow-md',
        warning: 'bg-warning/5 border-warning/20 hover:bg-warning/10 hover:shadow-md',
        error: 'bg-error/5 border-error/20 hover:bg-error/10 hover:shadow-md',
      },
      clickable: {
        true: 'cursor-pointer hover:shadow-lg hover:-translate-y-0.5',
        false: '',
      },
    },
    defaultVariants: {
      variant: 'default',
      clickable: false,
    },
  }
)

export interface StatsCardProps extends VariantProps<typeof statsCardVariants> {
  icon: string
  label: string
  value: string | number
  subtitle?: string
  trend?: {
    value: number
    isPositive: boolean
  }
  onClick?: () => void
  className?: string
}

/**
 * Statistics card component for displaying metrics
 *
 * @example
 * <StatsCard
 *   icon="⚡"
 *   label="决策分析"
 *   value="1,234"
 *   subtitle="待处理: 56"
 * />
 * <StatsCard
 *   icon="📊"
 *   label="响应时间"
 *   value="120ms"
 *   trend={{ value: 12, isPositive: true }}
 *   variant="success"
 * />
 */
export function StatsCard({
  icon,
  label,
  value,
  subtitle,
  trend,
  variant,
  onClick,
  className,
}: StatsCardProps) {
  return (
    <div
      className={cn(
        statsCardVariants({ variant, clickable: !!onClick }),
        onClick && 'cursor-pointer',
        className
      )}
      onClick={onClick}
    >
      <span className="text-2xl flex-shrink-0">{icon}</span>
      <div className="flex-1 min-w-0">
        <div className="flex items-baseline gap-2">
          <div className="text-2xl font-bold leading-none truncate">{value}</div>
          {trend && (
            <div
              className={cn(
                'flex items-center gap-0.5 text-xs font-medium',
                trend.isPositive ? 'text-success' : 'text-error'
              )}
            >
              {trend.isPositive ? (
                <TrendingUp className="w-3 h-3" />
              ) : (
                <TrendingDown className="w-3 h-3" />
              )}
              {trend.value}%
            </div>
          )}
        </div>
        <div className="text-xs text-muted-foreground mt-1">{label}</div>
        {subtitle && (
          <div className="text-xs text-muted-foreground/60 truncate">{subtitle}</div>
        )}
      </div>
    </div>
  )
}

/**
 * Compact stats card for smaller spaces
 */
export interface StatsCardCompactProps {
  label: string
  value: string | number
  change?: number
  className?: string
}

export function StatsCardCompact({ label, value, change, className }: StatsCardCompactProps) {
  return (
    <div className={cn('flex items-center justify-between p-3 rounded-md border bg-card/50', className)}>
      <span className="text-sm text-muted-foreground">{label}</span>
      <div className="flex items-center gap-2">
        <span className="font-semibold">{value}</span>
        {change !== undefined && (
          <span
            className={cn(
              'text-xs',
              change > 0 ? 'text-success' : change < 0 ? 'text-error' : 'text-muted-foreground'
            )}
          >
            {change > 0 ? '+' : ''}{change}%
          </span>
        )}
      </div>
    </div>
  )
}
