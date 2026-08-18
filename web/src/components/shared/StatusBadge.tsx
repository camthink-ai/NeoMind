import { cva, type VariantProps } from 'class-variance-authority'
import { getStatusColor } from '@/lib/utils/status'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'
import { textNano } from "@/design-system/tokens/typography"

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
        sm: 'px-1.5 py-0.5 text-nano', // textNano token - kept as static string for CVA variant
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
  const { t } = useTranslation('common')
  const color = getStatusColor(status)
  const statusLower = status.toLowerCase()
  const label = t(`statusLabels.${statusLower}`, { defaultValue: status })
  const isOnline = ['online', 'connected', 'active'].includes(statusLower)

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
    critical: { label: t('alertLevels.critical'), className: 'bg-error-light text-error border-error-light' },
    warning: { label: t('alertLevels.warning'), className: 'bg-warning-light text-warning border-warning-light' },
    info: { label: t('alertLevels.info'), className: 'bg-info-light text-info border-info-light' },
    emergency: { label: t('alertLevels.emergency'), className: 'bg-error-light text-error border-error-light' },
  }

  const { label, className: levelClass } = config[level]

  return (
    <span className={cn('inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs font-medium border', levelClass, className)}>
      {label}
    </span>
  )
}
