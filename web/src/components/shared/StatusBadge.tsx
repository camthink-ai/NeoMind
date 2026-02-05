import { cva, type VariantProps } from 'class-variance-authority'
import { getStatusColor, getStatusLabel } from '@/lib/utils/status'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'

const badgeVariants = cva(
  'inline-flex items-center gap-1.5 px-2 py-0.5 rounded-md text-xs font-medium transition-colors',
  {
    variants: {
      variant: {
        success: 'badge-success',
        warning: 'badge-warning',
        error: 'badge-error',
        info: 'badge-info',
        muted: 'bg-muted text-muted-foreground',
      },
      size: {
        sm: 'px-1.5 py-0.5 text-[10px]',
        md: 'px-2 py-0.5 text-xs',
        lg: 'px-2.5 py-1 text-sm',
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
  className?: string
  showDot?: boolean
}

/**
 * Status badge component with automatic color mapping
 *
 * @example
 * <StatusBadge status="online" />
 * <StatusBadge status="pending" />
 * <StatusBadge status="failed" size="sm" showDot />
 */
export function StatusBadge({ status, className, showDot = true }: StatusBadgeProps) {
  const color = getStatusColor(status)
  const label = getStatusLabel(status)
  const isOnline = ['online', 'connected', 'active'].includes(status.toLowerCase())

  return (
    <span className={cn(badgeVariants({ variant: color }), className)}>
      {showDot && (
        <span
          className={cn(
            'w-1.5 h-1.5 rounded-full',
            isOnline ? 'bg-success animate-pulse' : 'bg-muted-foreground'
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
  const { t } = useTranslation('common')

  const config = {
    critical: { label: t('alertLevels.critical'), className: 'bg-error/10 text-error border-error/20' },
    warning: { label: t('alertLevels.warning'), className: 'bg-warning/10 text-warning border-warning/20' },
    info: { label: t('alertLevels.info'), className: 'bg-info/10 text-info border-info/20' },
    emergency: { label: t('alertLevels.emergency'), className: 'bg-red-600/10 text-red-600 border-red-600/20' },
  }

  const { label, className: levelClass } = config[level]

  return (
    <span className={cn('inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs font-medium border', levelClass, className)}>
      {label}
    </span>
  )
}
