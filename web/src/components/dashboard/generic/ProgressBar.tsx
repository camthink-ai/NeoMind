/**
 * Progress Bar Component (Redesigned)
 *
 * Clean, professional layout that works well across devices.
 * Horizontal layout with label, percentage, and progress bar.
 */

import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { dashboardComponentSize, dashboardCardBase } from '@/design-system/tokens/size'
import type { DataSourceOrList } from '@/types/dashboard'

export interface ProgressBarProps {
  dataSource?: DataSourceOrList
  value?: number
  max?: number
  label?: string
  size?: 'sm' | 'md' | 'lg'
  color?: string
  warningThreshold?: number
  dangerThreshold?: number
  showCard?: boolean
  variant?: 'default' | 'compact' | 'circular'
  className?: string
}

function safeToNumber(value: unknown): number {
  if (typeof value === 'number') return value
  if (typeof value === 'string') {
    const num = parseFloat(value)
    return isNaN(num) ? 0 : num
  }
  if (typeof value === 'boolean') return value ? 1 : 0
  return 0
}

// Get color for progress bar based on percentage
const getProgressColor = (
  percentage: number,
  customColor?: string,
  warningThreshold = 70,
  dangerThreshold = 90
): string => {
  if (customColor) return customColor

  if (percentage >= dangerThreshold) {
    return 'hsl(var(--destructive))'
  }
  if (percentage >= warningThreshold) {
    return 'hsl(45, 93%, 47%)' // amber-500
  }
  return 'hsl(var(--primary))'
}

// Get text color based on percentage
const getTextColor = (
  percentage: number,
  customColor?: string,
  warningThreshold = 70,
  dangerThreshold = 90
): string => {
  if (customColor) return customColor

  if (percentage >= dangerThreshold) {
    return 'text-destructive'
  }
  if (percentage >= warningThreshold) {
    return 'text-amber-600 dark:text-amber-500'
  }
  return 'text-primary'
}

export function ProgressBar({
  dataSource,
  value: propValue,
  max = 100,
  label,
  size = 'md',
  color,
  warningThreshold = 70,
  dangerThreshold = 90,
  showCard = true,
  variant = 'default',
  className,
}: ProgressBarProps) {
  // When dataSource is provided, don't use propValue as fallback
  const { data, loading, error } = useDataSource<number>(dataSource, {
    fallback: dataSource ? undefined : (propValue ?? 0)
  })
  const rawValue = error ? propValue : data ?? propValue ?? 0
  const value = safeToNumber(rawValue)
  const percentage = Math.min(100, Math.max(0, (value / max) * 100))

  const sizeConfig = dashboardComponentSize[size]
  const barHeight = size === 'sm' ? 'h-1.5' : size === 'md' ? 'h-2' : 'h-2.5'
  const progressColor = getProgressColor(percentage, color, warningThreshold, dangerThreshold)

  // Compact variant - just the progress bar
  if (variant === 'compact') {
    return (
      <div className={cn('w-full h-full flex items-center', className)}>
        {loading ? (
          <Skeleton className={cn('w-full h-full rounded-full', barHeight)} />
        ) : (
          <div className={cn('relative w-full h-full rounded-full bg-muted/40 overflow-hidden', barHeight)}>
            <div
              className={cn('h-full rounded-full transition-all duration-500 ease-out')}
              style={{ width: `${percentage}%`, backgroundColor: progressColor }}
            />
          </div>
        )}
      </div>
    )
  }

  // Circular variant
  if (variant === 'circular') {
    const radius = size === 'sm' ? 28 : size === 'md' ? 32 : 36
    const strokeWidth = size === 'sm' ? 3 : size === 'md' ? 3.5 : 4
    const circumference = 2 * Math.PI * (radius - strokeWidth / 2)
    const offset = circumference - (percentage / 100) * circumference

    return (
      <div className={cn(dashboardCardBase, 'flex flex-col items-center justify-center', sizeConfig.padding, className)}>
        {loading ? (
          <Skeleton className={cn('rounded-full', size === 'sm' ? 'h-16 w-16' : size === 'md' ? 'h-20 w-20' : 'h-24 w-24')} />
        ) : (
          <div className="relative">
            <svg className={cn('transform -rotate-90', size === 'sm' ? 'h-16 w-16' : size === 'md' ? 'h-20 w-20' : 'h-24 w-24')} viewBox={`0 0 ${radius * 2} ${radius * 2}`}>
              <circle
                cx={radius}
                cy={radius}
                r={radius - strokeWidth / 2}
                fill="none"
                stroke="hsl(var(--muted) / 0.3)"
                strokeWidth={strokeWidth}
              />
              <circle
                cx={radius}
                cy={radius}
                r={radius - strokeWidth / 2}
                fill="none"
                stroke={progressColor}
                strokeWidth={strokeWidth}
                strokeDasharray={circumference}
                strokeDashoffset={offset}
                strokeLinecap="round"
                className="transition-all duration-500 ease-out"
              />
            </svg>
            <div className="absolute inset-0 flex items-center justify-center">
              <span className={cn('font-bold text-foreground tabular-nums', sizeConfig.valueText)}>
                {Math.round(percentage)}%
              </span>
            </div>
          </div>
        )}
        {label && (
          <span className={cn('text-muted-foreground mt-2', sizeConfig.labelText)}>
            {label}
          </span>
        )}
      </div>
    )
  }

  // Default variant - horizontal layout: label | bar | percentage
  const content = (
    <div className="flex items-center gap-3 w-full min-h-0">
      {/* Label */}
      {label && (
        <span className={cn('text-muted-foreground shrink-0 truncate max-w-[80px]', sizeConfig.labelText)}>
          {label}
        </span>
      )}

      {/* Progress bar */}
      <div className={cn('flex-1 min-w-0 rounded-full bg-muted/40 overflow-hidden', barHeight)}>
        {loading ? (
          <Skeleton className={cn('h-full w-full rounded-full', barHeight)} />
        ) : (
          <div
            className="h-full rounded-full transition-all duration-500 ease-out"
            style={{ width: `${percentage}%`, backgroundColor: progressColor }}
          />
        )}
      </div>

      {/* Percentage */}
      {loading ? (
        <Skeleton className={cn('h-4 w-10 shrink-0 rounded')} />
      ) : (
        <span className={cn('font-semibold tabular-nums shrink-0', sizeConfig.labelText)}>
          {Math.round(percentage)}%
        </span>
      )}
    </div>
  )

  if (showCard) {
    return (
      <div className={cn(dashboardCardBase, 'flex items-center justify-center', sizeConfig.padding, className)}>
        {content}
      </div>
    )
  }

  return <div className={cn('w-full flex items-center justify-center', sizeConfig.padding, className)}>{content}</div>
}
