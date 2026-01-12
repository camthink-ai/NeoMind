import { cva, type VariantProps } from 'class-variance-authority'
import { getStatusColor, getStatusLabel } from '@/lib/utils/status'
import { cn } from '@/lib/utils'

const badgeVariants = cva(
  'inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs font-medium transition-colors',
  {
    variants: {
      variant: {
        success: 'bg-success/10 text-success border border-success/20',
        warning: 'bg-warning/10 text-warning border border-warning/20',
        error: 'bg-error/10 text-error border border-error/20',
        info: 'bg-info/10 text-info border border-info/20',
        muted: 'bg-muted/50 text-muted-foreground',
      },
      size: {
        sm: 'px-2 py-0.5 text-[10px]',
        md: 'px-2.5 py-1 text-xs',
        lg: 'px-3 py-1.5 text-sm',
      },
    },
    defaultVariants: {
      variant: 'muted',
      size: 'md',
    },
  }
)

export interface StatusBadgeProps extends VariantProps<typeof badgeVariants> {
  status: string
  showDot?: boolean
  className?: string
}

/**
 * Status badge component with automatic color mapping
 *
 * @example
 * <StatusBadge status="online" />
 * <StatusBadge status="pending" showDot />
 * <StatusBadge status="failed" size="sm" />
 */
export function StatusBadge({ status, showDot = true, size, className }: StatusBadgeProps) {
  const color = getStatusColor(status)
  const label = getStatusLabel(status)

  return (
    <span className={cn(badgeVariants({ variant: color, size }), className)}>
      {showDot && (
        <span
          className={cn(
            'h-1.5 w-1.5 rounded-full animate-pulse',
            color === 'success' && 'bg-success',
            color === 'warning' && 'bg-warning',
            color === 'error' && 'bg-error',
            color === 'info' && 'bg-info',
            color === 'muted' && 'bg-muted-foreground'
          )}
        />
      )}
      {label}
    </span>
  )
}

/**
 * Alert level badge for severity indicators
 */
export interface AlertBadgeProps {
  level: 'critical' | 'warning' | 'info' | 'emergency'
  className?: string
}

export function AlertBadge({ level, className }: AlertBadgeProps) {
  const config = {
    critical: { label: '严重', className: 'bg-error/10 text-error border-error/20' },
    warning: { label: '警告', className: 'bg-warning/10 text-warning border-warning/20' },
    info: { label: '信息', className: 'bg-info/10 text-info border-info/20' },
    emergency: { label: '紧急', className: 'bg-red-600/10 text-red-600 border-red-600/20' },
  }

  const { label, className: levelClass } = config[level]

  return (
    <span className={cn('inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs font-medium border', levelClass, className)}>
      {label}
    </span>
  )
}
